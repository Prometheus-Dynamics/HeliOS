//! The one Orion subscription behind every client: connect to the node's stream socket, subscribe
//! to the stream from the backfill on, feed what arrives to [`LiveStreams`], and resume after a
//! gap or a dropped connection (Orion `docs/streams.md`, "Resuming"). Nothing here polls.

use std::sync::Arc;
use std::time::Duration;

use orion::control_plane::{
    Backfill, ClientEventKind, SeriesKind, StatusSubject, StreamCursor, StreamRef, StreamSubscription,
};
use tokio::sync::oneshot;

use super::{LiveStreams, codec};

/// Wait between attempts to reach Orion's stream socket.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// What one Orion event means for the clients.
#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    /// The stream's series, as a catalog text frame, and whether its publisher is there.
    Catalog { text: String, live: bool },
    /// A batch, encoded.
    Samples(codec::SamplesFrame),
    /// Something was lost, or the position no longer holds: subscribe again from this cursor
    /// (`None`: from the backfill).
    Resubscribe(Option<StreamCursor>),
}

/// The state of one subscription: the kinds to decode with and the position reached.
#[derive(Debug, Default)]
pub struct Feed {
    kinds: Vec<SeriesKind>,
    /// The last batch taken, which the next one must follow.
    cursor: Option<StreamCursor>,
}

impl Feed {
    pub fn new(cursor: Option<StreamCursor>) -> Self {
        Self { kinds: Vec::new(), cursor }
    }

    pub fn cursor(&self) -> Option<StreamCursor> {
        self.cursor
    }

    /// Folds in one event of the subscription.
    pub fn on_event(&mut self, event: ClientEventKind) -> Step {
        match event {
            ClientEventKind::StreamCatalog(catalog) => {
                self.kinds = catalog.series.iter().map(|series| series.kind).collect();
                // Kept across a catalog: after a reconnect the resume point still holds, and a new
                // epoch fails the gap check on its own.
                let text = codec::catalog_text(&catalog);
                Step::Catalog { text, live: catalog.live }
            }
            ClientEventKind::StreamData(data) => {
                // A live batch that doesn't follow the one taken: a sample was lost.
                if !data.backfill
                    && let Some(seen) = self.cursor
                    && seen.epoch == data.cursor.epoch
                    && data.prev_seq != seen.seq
                {
                    return Step::Resubscribe(Some(seen));
                }
                self.cursor = Some(data.cursor);
                match codec::samples_frame(&data, &self.kinds) {
                    Some(frame) => Step::Samples(frame),
                    None => Step::Nothing,
                }
            }
            // The position no longer holds: a catalog and the backfill follow.
            ClientEventKind::StreamReset(_) => {
                self.cursor = None;
                Step::Nothing
            }
            _ => Step::Nothing,
        }
    }
}

/// Follows the node's stream until `stop` fires (or its sender is dropped), then unsubscribes.
/// `generation` is the one [`LiveStreams`] started this follower with: anything it reports after
/// a newer generation began is dropped there.
pub async fn run(live: Arc<LiveStreams>, generation: u64, mut stop: oneshot::Receiver<()>) {
    let mut cursor: Option<StreamCursor> = None;
    loop {
        live.report(generation, "connecting", "");
        let attempt = tokio::select! {
            _ = &mut stop => return,
            attempt = connect(&live, cursor) => attempt,
        };
        let (mut events, stream) = match attempt {
            Ok(connected) => connected,
            Err(reason) => {
                live.report(generation, "unavailable", &reason);
                tokio::select! {
                    _ = &mut stop => return,
                    _ = tokio::time::sleep(RECONNECT_DELAY) => continue,
                }
            }
        };
        live.report(generation, "waiting", &format!("subscribed to {}; no batch yet", stream.id));
        let mut feed = Feed::new(cursor);
        loop {
            let batch = tokio::select! {
                _ = &mut stop => {
                    let _ = events.unsubscribe_stream(stream.clone()).await;
                    return;
                }
                batch = events.next_events() => batch,
            };
            let Ok(batch) = batch else {
                live.report(generation, "unavailable", "Orion's stream connection ended");
                break;
            };
            let mut resubscribe = None;
            for event in batch {
                match feed.on_event(event.event) {
                    Step::Nothing => {}
                    Step::Catalog { text, live: publishing } => live.publish_catalog(generation, text, publishing),
                    Step::Samples(frame) => live.publish_samples(generation, frame),
                    Step::Resubscribe(from) => {
                        resubscribe = Some(from);
                        break;
                    }
                }
            }
            if let Some(from) = resubscribe {
                let backfill = from.map_or(Backfill::LastMs(live.backfill_ms()), Backfill::Since);
                if events.subscribe_stream(subscription(&stream, backfill)).await.is_err() {
                    live.report(generation, "unavailable", "Orion refused the resubscription");
                    break;
                }
            }
        }
        cursor = feed.cursor();
        tokio::select! {
            _ = &mut stop => return,
            _ = tokio::time::sleep(RECONNECT_DELAY) => {}
        }
    }
}

/// A subscription to the stream on this node, from `cursor` or the backfill.
fn subscription(stream: &StreamRef, backfill: Backfill) -> StreamSubscription {
    StreamSubscription::all(stream.clone()).with_backfill(backfill)
}

/// Connects to the node's stream socket and subscribes to its live stream, owned by the node.
async fn connect(
    live: &LiveStreams,
    cursor: Option<StreamCursor>,
) -> Result<(orion::client::ControlPlaneEventStream, StreamRef), String> {
    let mut events = live.orion().live_stream_events().await.map_err(|error| error.message)?;
    let stream = StreamRef { owner: StatusSubject::Node(events.node_id().clone()), id: live.stream_id().to_string() };
    let backfill = cursor.map_or(Backfill::LastMs(live.backfill_ms()), Backfill::Since);
    events
        .subscribe_stream(subscription(&stream, backfill))
        .await
        .map_err(|error| format!("Orion refused the subscription: {error}"))?;
    Ok((events, stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orion::control_plane::{SampleChunkBuilder, SeriesDecl, StreamCatalog, StreamCursor, StreamData, StreamReset, StreamResetReason};

    fn node() -> StatusSubject {
        "node/raze-1".parse().expect("a node subject")
    }

    fn catalog() -> StreamCatalog {
        StreamCatalog {
            stream: StreamRef { owner: node(), id: "board.hardware".into() },
            epoch: 7,
            series: vec![SeriesDecl::new("imu.acceleration.x", SeriesKind::F32, 10_000), SeriesDecl::new("light.led.0", SeriesKind::U32, 50_000)],
            live: true,
        }
    }

    fn batch(epoch: u64, seq: u64, prev_seq: u64, backfill: bool, t_us: u64) -> StreamData {
        let mut builder = SampleChunkBuilder::new(0, SeriesKind::F32);
        builder.push(t_us, 9.5);
        StreamData {
            stream: StreamRef { owner: node(), id: "board.hardware".into() },
            cursor: StreamCursor { epoch, seq },
            prev_seq,
            backfill,
            chunks: vec![builder.take()],
        }
    }

    #[test]
    fn a_catalog_is_text_and_hands_back_the_kinds() {
        let mut feed = Feed::new(None);
        let Step::Catalog { text, live } = feed.on_event(ClientEventKind::StreamCatalog(catalog())) else {
            panic!("a catalog step");
        };
        assert!(live);
        assert_eq!(feed.kinds, vec![SeriesKind::F32, SeriesKind::U32]);
        assert!(text.contains("\"owner\":\"node/raze-1\""), "{text}");
        assert!(text.contains("\"name\":\"light.led.0\""), "{text}");
    }

    #[test]
    fn batches_advance_the_cursor_and_gaps_resubscribe_from_it() {
        let mut feed = Feed::new(None);
        feed.on_event(ClientEventKind::StreamCatalog(catalog()));
        assert!(matches!(feed.on_event(ClientEventKind::StreamData(batch(7, 1, 0, true, 1_000))), Step::Samples(_)));
        assert_eq!(feed.cursor(), Some(StreamCursor { epoch: 7, seq: 1 }));
        // The next batch names 2 as its predecessor, not 1: batch 2 was lost.
        let gap = feed.on_event(ClientEventKind::StreamData(batch(7, 3, 2, false, 2_000)));
        assert_eq!(gap, Step::Resubscribe(Some(StreamCursor { epoch: 7, seq: 1 })));
        assert_eq!(feed.cursor(), Some(StreamCursor { epoch: 7, seq: 1 }), "a gap keeps the old position");
    }

    #[test]
    fn a_contiguous_batch_is_not_a_gap() {
        let mut feed = Feed::new(None);
        feed.on_event(ClientEventKind::StreamCatalog(catalog()));
        feed.on_event(ClientEventKind::StreamData(batch(7, 1, 0, true, 1_000)));
        assert!(matches!(feed.on_event(ClientEventKind::StreamData(batch(7, 2, 1, false, 2_000))), Step::Samples(_)));
    }

    #[test]
    fn a_catalog_keeps_the_position_and_a_reset_forgets_it() {
        let mut feed = Feed::new(Some(StreamCursor { epoch: 6, seq: 40 }));
        feed.on_event(ClientEventKind::StreamCatalog(catalog()));
        assert_eq!(feed.cursor(), Some(StreamCursor { epoch: 6, seq: 40 }));
        feed.on_event(ClientEventKind::StreamReset(StreamReset { stream: catalog().stream, reason: StreamResetReason::OutOfRange }));
        assert_eq!(feed.cursor(), None);
    }

    #[test]
    fn a_batch_before_any_catalog_has_no_kinds_and_is_dropped() {
        let mut feed = Feed::new(None);
        assert_eq!(feed.on_event(ClientEventKind::StreamData(batch(7, 1, 0, false, 1_000))), Step::Nothing);
    }
}
