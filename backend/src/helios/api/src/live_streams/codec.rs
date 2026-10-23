//! The frames `GET /v1/streams/ws` sends (see `live_streams`).
//!
//! - A **catalog** is a text frame: JSON, `{"type": "catalog", ...}`, with the series in the order
//!   the sample frames index them.
//! - **Samples** are a binary frame, all little endian, one block per series with samples:
//!
//! ```text
//! u8   frame kind (1: samples)
//! u8   flags (bit 0: Orion's backfill, the history a subscription starts with)
//! u16  number of blocks
//! per block:
//!   u16  series index (in the catalog)
//!   u32  number of samples n
//!   n x  f64  time, microseconds since the Unix epoch (exact below 2^53)
//!   n x  f64  value (a U32 colour is exact too)
//! ```
//!
//! Values are f64 on purpose: one decoder for every kind, and the colours (`0xWWRRGGBB`) and
//! times survive exactly. A block costs 16 bytes a sample.

use orion::control_plane::{SeriesKind, StreamCatalog, StreamData};
use serde::Serialize;

/// The binary frame kind of a samples frame.
pub const SAMPLES_FRAME: u8 = 1;
/// The flag of samples Orion sent as the subscription's backfill.
pub const FLAG_BACKFILL: u8 = 1;

/// Samples of one stream batch, encoded, and the newest sample's time.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplesFrame {
    pub bytes: Vec<u8>,
    /// The newest sample's time (µs), which orders the history kept for new clients.
    pub newest_us: u64,
}

#[derive(Serialize)]
struct CatalogFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    stream: &'a str,
    owner: String,
    epoch: u64,
    live: bool,
    series: Vec<SeriesEntry<'a>>,
}

#[derive(Serialize)]
struct SeriesEntry<'a> {
    name: &'a str,
    unit: Option<&'a str>,
    kind: &'static str,
    period_us: u32,
}

/// The kind's name in the catalog.
pub fn kind_name(kind: SeriesKind) -> &'static str {
    match kind {
        SeriesKind::F32 => "f32",
        SeriesKind::F64 => "f64",
        SeriesKind::I64 => "i64",
        SeriesKind::U32 => "u32",
    }
}

/// The catalog as a text frame.
pub fn catalog_text(catalog: &StreamCatalog) -> String {
    let frame = CatalogFrame {
        kind: "catalog",
        stream: &catalog.stream.id,
        owner: catalog.stream.owner.to_string(),
        epoch: catalog.epoch,
        live: catalog.live,
        series: catalog
            .series
            .iter()
            .map(|series| SeriesEntry {
                name: &series.name,
                unit: series.unit.as_deref(),
                kind: kind_name(series.kind),
                period_us: series.period_us,
            })
            .collect(),
    };
    // Plain strings and numbers always serialize.
    serde_json::to_string(&frame).unwrap_or_else(|_| String::from("{\"type\":\"catalog\",\"series\":[]}"))
}

/// One stream batch as a samples frame: every chunk whose series the catalog names (`kinds`, in
/// the catalog's order). `None` when no chunk holds a sample.
pub fn samples_frame(data: &StreamData, kinds: &[SeriesKind]) -> Option<SamplesFrame> {
    let mut blocks: Vec<(u16, Vec<(u64, f64)>)> = Vec::new();
    for chunk in &data.chunks {
        let Some(&kind) = kinds.get(usize::from(chunk.series)) else {
            continue;
        };
        // A chunk that doesn't decode is skipped; the rest of the batch still goes out.
        let Ok(samples) = chunk.samples(kind) else {
            continue;
        };
        if !samples.is_empty() {
            blocks.push((chunk.series, samples));
        }
    }
    if blocks.is_empty() {
        return None;
    }
    let newest_us = blocks.iter().flat_map(|(_, samples)| samples.iter().map(|(t, _)| *t)).max().unwrap_or(0);
    let total: usize = blocks.iter().map(|(_, samples)| samples.len()).sum();
    let mut bytes = Vec::with_capacity(4 + blocks.len() * 6 + total * 16);
    bytes.push(SAMPLES_FRAME);
    bytes.push(if data.backfill { FLAG_BACKFILL } else { 0 });
    bytes.extend_from_slice(&u16::try_from(blocks.len()).unwrap_or(u16::MAX).to_le_bytes());
    for (series, samples) in &blocks {
        bytes.extend_from_slice(&series.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(samples.len()).unwrap_or(u32::MAX).to_le_bytes());
        for (t, _) in samples {
            bytes.extend_from_slice(&(*t as f64).to_le_bytes());
        }
        for (_, value) in samples {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Some(SamplesFrame { bytes, newest_us })
}

#[cfg(test)]
pub mod test_support {
    /// One decoded block: a series index and its (time µs, value) samples.
    #[derive(Debug, PartialEq)]
    pub struct Block {
        pub series: u16,
        pub samples: Vec<(f64, f64)>,
    }

    /// A samples frame read back: its flags and blocks. `None` when it is malformed.
    pub fn decode(bytes: &[u8]) -> Option<(u8, Vec<Block>)> {
        if *bytes.first()? != super::SAMPLES_FRAME {
            return None;
        }
        let flags = *bytes.get(1)?;
        let blocks = usize::from(u16::from_le_bytes(bytes.get(2..4)?.try_into().ok()?));
        let mut at = 4;
        let mut out = Vec::with_capacity(blocks);
        for _ in 0..blocks {
            let series = u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?);
            let n = u32::from_le_bytes(bytes.get(at + 2..at + 6)?.try_into().ok()?) as usize;
            at += 6;
            let times = bytes.get(at..at + n * 8)?;
            let values = bytes.get(at + n * 8..at + n * 16)?;
            at += n * 16;
            let read = |slice: &[u8], i: usize| f64::from_le_bytes(slice[i * 8..i * 8 + 8].try_into().unwrap_or([0; 8]));
            let samples = (0..n).map(|i| (read(times, i), read(values, i))).collect();
            out.push(Block { series, samples });
        }
        (at == bytes.len()).then_some((flags, out))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::decode;
    use super::*;
    use orion::control_plane::{SampleChunkBuilder, SeriesDecl, StatusSubject, StreamCursor, StreamRef};

    fn stream(id: &str) -> StreamRef {
        StreamRef { owner: "node/raze-1".parse::<StatusSubject>().expect("a node subject"), id: id.into() }
    }

    fn data(chunks: Vec<orion::control_plane::SampleChunk>, backfill: bool) -> StreamData {
        StreamData { stream: stream("board.hardware"), cursor: StreamCursor { epoch: 2, seq: 9 }, prev_seq: 8, backfill, chunks }
    }

    #[test]
    fn orion_chunks_come_back_as_the_samples_they_were_built_from() {
        // An F32 channel at 100 Hz and a light's U32 colours (0xWWRRGGBB, exact as f64).
        let mut accel = SampleChunkBuilder::new(0, SeriesKind::F32);
        let mut led = SampleChunkBuilder::new(1, SeriesKind::U32);
        for i in 0..5u64 {
            accel.push(1_700_000_000_000_000 + i * 10_000, 9.5 + i as f64 * 0.25);
        }
        led.push(1_700_000_000_000_000, f64::from(0xff00_ff80_u32));
        let frame = samples_frame(&data(vec![accel.take(), led.take()], true), &[SeriesKind::F32, SeriesKind::U32]).expect("samples");
        assert_eq!(frame.newest_us, 1_700_000_000_040_000);
        let (flags, blocks) = decode(&frame.bytes).expect("a well-formed frame");
        assert_eq!(flags, FLAG_BACKFILL);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].series, 0);
        assert_eq!(blocks[0].samples.len(), 5);
        assert_eq!(blocks[0].samples[4], (1_700_000_000_040_000.0, 10.5));
        assert_eq!(blocks[1].series, 1);
        assert_eq!(blocks[1].samples, vec![(1_700_000_000_000_000.0, f64::from(0xff00_ff80_u32))]);
    }

    #[test]
    fn a_chunk_of_an_unknown_series_or_a_bad_one_is_skipped_and_the_rest_goes_out() {
        let mut known = SampleChunkBuilder::new(0, SeriesKind::F32);
        known.push(5_000, 1.0);
        let mut unknown = SampleChunkBuilder::new(7, SeriesKind::F32);
        unknown.push(5_000, 2.0);
        // Encoded as U32 but declared F32 in the catalog: its bytes do not decode as F32 samples.
        let mut mismatched = SampleChunkBuilder::new(1, SeriesKind::U32);
        mismatched.push(5_000, 3.0);
        let mut bad = mismatched.take();
        bad.data.push(0xff);
        let frame = samples_frame(&data(vec![known.take(), unknown.take(), bad], false), &[SeriesKind::F32, SeriesKind::F32]).expect("samples");
        let (flags, blocks) = decode(&frame.bytes).expect("frame");
        assert_eq!(flags, 0, "a live batch");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].samples, vec![(5_000.0, 1.0)]);
    }

    #[test]
    fn a_batch_with_no_samples_has_no_frame() {
        let empty = SampleChunkBuilder::new(0, SeriesKind::F32).take();
        assert_eq!(samples_frame(&data(vec![empty], false), &[SeriesKind::F32]), None);
    }

    #[test]
    fn the_catalog_names_the_owner_series_kinds_and_units() {
        let catalog = StreamCatalog {
            stream: stream("board.hardware"),
            epoch: 3,
            series: vec![
                SeriesDecl::new("imu.acceleration.x", SeriesKind::F32, 10_000).with_unit("m/s²"),
                SeriesDecl::new("light.led.0", SeriesKind::U32, 50_000),
            ],
            live: false,
        };
        let text = catalog_text(&catalog);
        let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(value["type"], "catalog");
        assert_eq!(value["owner"], "node/raze-1");
        assert_eq!(value["epoch"], 3);
        assert_eq!(value["live"], false);
        assert_eq!(value["series"][0]["name"], "imu.acceleration.x");
        assert_eq!(value["series"][0]["unit"], "m/s²");
        assert_eq!(value["series"][0]["kind"], "f32");
        assert_eq!(value["series"][1]["unit"], serde_json::Value::Null);
        assert_eq!(value["series"][1]["kind"], "u32");
        assert_eq!(value["series"][1]["period_us"], 50_000);
    }
}
