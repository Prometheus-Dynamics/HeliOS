use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use lib_runtime_policy::{ResolvedEngineFlamegraphPolicy, HELIOS_ENGINE_FLAMEGRAPH_POLICY};

#[derive(Debug, Clone, Default)]
pub struct FlamegraphCapture {
    pub path: String,
    pub size_bytes: u64,
    pub captured_at_ms: u64,
}

#[cfg(feature = "pprof")]
pub struct FlamegraphGuard {
    guard: pprof::ProfilerGuard<'static>,
    path: PathBuf,
}

#[cfg(not(feature = "pprof"))]
pub struct FlamegraphGuard;

#[cfg(feature = "pprof")]
impl FlamegraphGuard {
    pub fn start(path: PathBuf) -> io::Result<Self> {
        let mut builder = pprof::ProfilerGuardBuilder::default();
        let freq = flamegraph_policy().frequency;
        if let Some(freq) = freq {
            builder = builder.frequency(freq);
        }
        let guard = builder.blocklist(&["libc", "libgcc", "pthread"]).build().map_err(io::Error::other)?;
        Ok(Self { guard, path })
    }

    pub fn finish(self) -> io::Result<FlamegraphCapture> {
        let report = self.guard.report().build().map_err(io::Error::other)?;
        let mut file = std::fs::File::create(&self.path)?;
        report.flamegraph(&mut file).map_err(io::Error::other)?;
        let size_bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let captured_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Ok(FlamegraphCapture { path: self.path.display().to_string(), size_bytes, captured_at_ms })
    }
}

#[cfg(not(feature = "pprof"))]
impl FlamegraphGuard {
    pub fn start(_path: PathBuf) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pprof feature not enabled"))
    }

    pub fn finish(self) -> io::Result<FlamegraphCapture> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pprof feature not enabled"))
    }
}

fn flamegraph_policy() -> &'static ResolvedEngineFlamegraphPolicy {
    static VALUE: std::sync::OnceLock<ResolvedEngineFlamegraphPolicy> = std::sync::OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_FLAMEGRAPH_POLICY.resolve())
}

pub fn build_flamegraph_path(call_idx: u64) -> PathBuf {
    if let Some(path) = flamegraph_policy().path.as_ref() {
        return path.clone();
    }
    let dir = flamegraph_policy().dir.clone().unwrap_or_else(std::env::temp_dir);
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    dir.join(format!("helios-flamegraph-{call_idx}-{ts}.svg"))
}
