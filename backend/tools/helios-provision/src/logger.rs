use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Logger {
    path: PathBuf,
    verbose: bool,
}

impl Logger {
    pub fn new(path: impl AsRef<Path>, verbose: bool) -> Self {
        let path = path.as_ref().to_path_buf();
        let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new("/")));
        Self { path, verbose }
    }

    pub fn log(&mut self, msg: impl AsRef<str>) {
        let line = format!("[provision] {}", msg.as_ref());
        println!("{line}");
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{line}");
        }
        if self.verbose {
            eprintln!("{line}");
        }
    }
}
