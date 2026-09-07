//! Persistent UTF-8 transcript for one run, independent of the UI's short tail.
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub(super) struct RunLog {
    path: Arc<PathBuf>,
    file: Arc<Mutex<File>>,
}

impl RunLog {
    pub(super) fn create(directory: &Path) -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let path = std::path::absolute(directory)?
            .join(format!("bevyistr_run_{}_{stamp}.log", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let log = Self {
            path: Arc::new(path),
            file: Arc::new(Mutex::new(file)),
        };
        log.line("bevyistr - FrontISTR run log")?;
        log.line(&format!("Working directory: {}", directory.display()))?;
        Ok(log)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn line(&self, text: &str) -> io::Result<()> {
        let mut file = self.file.lock().unwrap_or_else(|p| p.into_inner());
        writeln!(file, "{text}")?;
        file.flush()
    }

    /// Published only after output readers have joined and the final status
    /// has been written. The viewer checks this before its final drain.
    pub(super) fn finish(&self) -> io::Result<()> {
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(completion_path(self.path()))?;
        marker.write_all(b"complete\n")
    }
}

pub(super) fn completion_path(log: &Path) -> PathBuf {
    let mut path = log.as_os_str().to_owned();
    path.push(".done");
    PathBuf::from(path)
}
