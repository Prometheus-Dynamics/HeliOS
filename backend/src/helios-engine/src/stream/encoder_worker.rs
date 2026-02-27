use crate::error::{Error, Result};
use crate::ipc::EncoderSettings;
use crate::stream::EncodedFrame;
use crate::stream::ShmemWriter;
use metrics::histogram;
use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use styx::codec::ffmpeg::{FfmpegH264Encoder, FfmpegH265Encoder, FfmpegMjpegEncoder, FfmpegVideoEncoder};
use styx::codec::Codec;
use styx::prelude::{BufferPool, ColorSpace, FourCc, FrameLease, FrameMeta, MediaFormat, Resolution};
use tokio::sync::broadcast;

pub struct EncoderWorkerStart {
    pub stream_label: String,
    pub encoder: Arc<dyn Codec>,
    pub encode_input: FourCc,
    pub encode_output: FourCc,
    pub encoder_settings: Option<EncoderSettings>,
    pub shmem: Option<ShmemWriter>,
    pub encoded_tx: broadcast::Sender<EncodedFrame>,
    pub encoder_stats: styx::codec::CodecStats,
    pub activity_ms: Arc<AtomicU64>,
}

#[allow(clippy::large_enum_variant)]
enum EncoderJob {
    Image { image: Arc<image::DynamicImage>, ts: u64 },
    Stop,
}

pub struct EncoderWorker {
    tx: SyncSender<EncoderJob>,
    pending: Arc<Mutex<Option<EncoderJob>>>,
    join: Option<JoinHandle<Option<ShmemWriter>>>,
}

impl EncoderWorker {
    pub fn start(start: EncoderWorkerStart) -> Result<Self> {
        let (tx, rx) = sync_channel::<EncoderJob>(1);
        let pending = Arc::new(Mutex::new(None));

        let EncoderWorkerStart { stream_label, encoder, encode_input, encode_output, encoder_settings, shmem, encoded_tx, encoder_stats, activity_ms } = start;
        let thread_name = format!("helios-encoder-{stream_label}");

        let mut runtime = EncoderWorkerRuntime {
            stream_label: stream_label.into(),
            encoder,
            encode_input,
            encode_output,
            encoder_settings,
            encoded_tx,
            encoder_stats,
            activity_ms,
            shmem,
            pending: pending.clone(),
            pool: BufferPool::with_capacity(1, 1),
            pool_capacity: 1,
        };

        let join = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                runtime.run_loop(rx);
                runtime.shmem
            })
            .map_err(|_| Error::InvalidState("encoder worker thread spawn failed"))?;

        Ok(Self { tx, pending, join: Some(join) })
    }

    pub fn try_send_image(&self, image: Arc<image::DynamicImage>, ts: u64) -> bool {
        match self.tx.try_send(EncoderJob::Image { image, ts }) {
            Ok(()) => true,
            Err(TrySendError::Full(job)) => {
                if let Ok(mut pending) = self.pending.lock() {
                    *pending = Some(job);
                }
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn stop(mut self) -> Option<ShmemWriter> {
        let _ = self.tx.send(EncoderJob::Stop);
        self.join.take().and_then(|j| j.join().ok()).flatten()
    }
}

struct EncoderWorkerRuntime {
    stream_label: metrics::SharedString,
    encoder: Arc<dyn Codec>,
    encode_input: FourCc,
    encode_output: FourCc,
    encoder_settings: Option<EncoderSettings>,
    encoded_tx: broadcast::Sender<EncodedFrame>,
    encoder_stats: styx::codec::CodecStats,
    activity_ms: Arc<AtomicU64>,
    shmem: Option<ShmemWriter>,
    pending: Arc<Mutex<Option<EncoderJob>>>,
    pool: BufferPool,
    pool_capacity: usize,
}

impl EncoderWorkerRuntime {
    fn unix_now_ms() -> u64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_millis().min(u64::MAX as u128) as u64,
            Err(_) => 0,
        }
    }

    fn apply_encoder_settings(&mut self) {
        let Some(settings) = self.encoder_settings.as_ref() else {
            return;
        };

        let any = self.encoder.as_ref() as &dyn Any;
        let ffmpeg = any
            .downcast_ref::<FfmpegVideoEncoder>()
            .or_else(|| any.downcast_ref::<FfmpegH264Encoder>().map(|enc| &enc.0))
            .or_else(|| any.downcast_ref::<FfmpegH265Encoder>().map(|enc| &enc.0))
            .or_else(|| any.downcast_ref::<FfmpegMjpegEncoder>().map(|enc| &enc.0));

        let Some(ffmpeg) = ffmpeg else {
            return;
        };

        if let Some(bitrate) = settings.bitrate.filter(|value| *value > 0) {
            ffmpeg.set_bitrate(bitrate);
        }
        if let Some(gop) = settings.gop.filter(|value| *value > 0) {
            ffmpeg.set_gop(Some(gop));
        }
        if let Some(rate) = settings.framerate.as_ref().and_then(|rate| if rate.numerator > 0 && rate.denominator > 0 { Some((rate.numerator, rate.denominator)) } else { None }) {
            ffmpeg.set_framerate(Some(rate));
        }
        if let Some(output_resolution) = settings.output_resolution.as_ref().and_then(|resolution| Resolution::new(resolution.width, resolution.height)) {
            ffmpeg.set_output_resolution(Some(output_resolution));
        }
    }

    fn ensure_rgb24_pool(&mut self, bytes: usize) -> &BufferPool {
        let need = bytes.max(1);
        if self.pool_capacity < need {
            self.pool = BufferPool::with_capacity(1, need);
            self.pool_capacity = need;
        }
        &self.pool
    }

    fn dynamic_image_to_rgb24_frame(&mut self, image: &image::DynamicImage, ts: u64) -> Option<FrameLease> {
        let width = image.width().max(1);
        let height = image.height().max(1);
        let resolution = Resolution::new(width, height)?;
        let bytes = width as usize * height as usize * 3;
        let pool = self.ensure_rgb24_pool(bytes);
        let mut lease = pool.lease();
        lease.resize(bytes);

        let out = lease.as_mut_slice();
        if !write_rgb24(image, out) {
            return None;
        }

        let stride = (width as usize * 3).max(1);
        let meta = FrameMeta::new(MediaFormat::new(FourCc::new(*b"RG24"), resolution, ColorSpace::Unknown), ts);
        Some(FrameLease::single_plane(meta, lease, bytes, stride))
    }

    fn run_loop(&mut self, rx: Receiver<EncoderJob>) {
        self.apply_encoder_settings();
        let _ = self.encoder_settings.take();
        while let Ok(job) = rx.recv() {
            if !self.process_job(job) {
                break;
            }
            // Drain any pending "latest" job that replaced queued work while we were encoding.
            loop {
                let next = self.pending.lock().ok().and_then(|mut pending| pending.take());
                let Some(next) = next else {
                    break;
                };
                if !self.process_job(next) {
                    return;
                }
            }
        }
    }

    fn process_job(&mut self, job: EncoderJob) -> bool {
        match job {
            EncoderJob::Stop => return false,
            EncoderJob::Image { image, ts } => {
                let encode_start = Instant::now();
                if self.encode_input != FourCc::new(*b"RG24") {
                    self.encoder_stats.inc_errors();
                    tracing::warn!(
                        encode_input = ?self.encode_input,
                        "encoder worker received DynamicImage but encoder input is not RG24"
                    );
                    return true;
                }
                if let Some(out_frame) = self.dynamic_image_to_rgb24_frame(&image, ts) {
                    match self.encoder.process(out_frame) {
                        Ok(encoded_frame) => {
                            self.encoder_stats.inc_processed();
                            self.encoder_stats.record_duration(encode_start.elapsed());
                            if let Some(plane) = encoded_frame.planes().first() {
                                self.activity_ms.store(Self::unix_now_ms(), Ordering::Relaxed);
                                let data = Arc::<[u8]>::from(plane.data());
                                if let Some(shmem) = self.shmem.as_mut() {
                                    let res = encoded_frame.meta().format.resolution;
                                    let dims = (res.width.get(), res.height.get());
                                    let fourcc = encoded_frame.meta().format.code;
                                    if fourcc != self.encode_output {
                                        tracing::warn!(
                                            want = ?self.encode_output,
                                            got = ?fourcc,
                                            "encoder output fourcc mismatch; shmem tagged with actual output"
                                        );
                                    }
                                    if let Err(err) = shmem.write(Some(ts), Some(fourcc), dims, data.as_ref()) {
                                        tracing::warn!(path = ?shmem.path(), error = %err, "shmem write failed");
                                    }
                                }
                                let encode_ms = encode_start.elapsed().as_secs_f64() * 1000.0;
                                histogram!("helios.stream.encode_ms", "stream" => self.stream_label.clone()).record(encode_ms);
                                let _ = self.encoded_tx.send(EncodedFrame { data, ts_ms: Self::unix_now_ms() });
                            } else {
                                self.encoder_stats.inc_errors();
                                tracing::warn!("encoded frame had no planes");
                            }
                        }
                        Err(styx::codec::CodecError::Backpressure) => {
                            self.encoder_stats.inc_backpressure();
                        }
                        Err(err) => {
                            self.encoder_stats.inc_errors();
                            tracing::warn!(error = ?err, fourcc = ?self.encode_output, "frame encode failed");
                        }
                    }
                } else {
                    self.encoder_stats.inc_errors();
                    tracing::warn!("encode: dynamic image conversion failed");
                }
            }
        }
        true
    }
}

fn write_rgb24(image: &image::DynamicImage, out: &mut [u8]) -> bool {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let want = width.saturating_mul(height).saturating_mul(3);
    if out.len() < want {
        return false;
    }

    match image {
        image::DynamicImage::ImageRgb8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != want {
                return false;
            }
            out[..want].copy_from_slice(raw);
            true
        }
        image::DynamicImage::ImageRgba8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height).saturating_mul(4) {
                return false;
            }
            let mut di = 0usize;
            for src in raw.chunks_exact(4) {
                out[di] = src[0];
                out[di + 1] = src[1];
                out[di + 2] = src[2];
                di += 3;
            }
            true
        }
        image::DynamicImage::ImageLuma8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height) {
                return false;
            }
            let mut di = 0usize;
            for &g in raw {
                out[di] = g;
                out[di + 1] = g;
                out[di + 2] = g;
                di += 3;
            }
            true
        }
        image::DynamicImage::ImageLumaA8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height).saturating_mul(2) {
                return false;
            }
            let mut di = 0usize;
            for src in raw.chunks_exact(2) {
                let g = src[0];
                out[di] = g;
                out[di + 1] = g;
                out[di + 2] = g;
                di += 3;
            }
            true
        }
        _ => {
            // Slow fallback for uncommon variants (16-bit, etc).
            let rgb = image.to_rgb8();
            let raw = rgb.as_raw();
            if raw.len() != want {
                return false;
            }
            out[..want].copy_from_slice(raw);
            true
        }
    }
}
