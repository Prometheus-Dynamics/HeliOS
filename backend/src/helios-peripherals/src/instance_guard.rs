use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

/// Guards the helios-peripherals runtime to ensure only a single instance is active.
pub struct InstanceGuard {
    _lock: Flock<std::fs::File>,
}

impl InstanceGuard {
    /// Attempts to acquire the single-instance guard by taking an exclusive lock on `path`.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).read(true).write(true).truncate(false).mode(0o600).open(path)?;
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => Ok(Self { _lock: lock }),
            Err((file, errno)) => {
                drop(file);
                Err(io_error_from_errno(errno))
            }
        }
    }
}

fn io_error_from_errno(errno: Errno) -> io::Error {
    io::Error::from_raw_os_error(errno as i32)
}
