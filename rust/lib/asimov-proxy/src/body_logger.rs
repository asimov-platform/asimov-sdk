// This is free and unencumbered software released into the public domain.

//! Optional append-only request/response body logging.

use alloc::sync::Arc;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write as _},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

/// Appends request bodies and streamed response chunks to a file.
///
/// Clones share the same file. Writes are synchronous and best-effort: log
/// failures do not interrupt proxy responses.
#[derive(Clone)]
pub struct BodyLogger {
    file: Arc<Mutex<File>>,
}

impl BodyLogger {
    /// Appends to a log, creating new files with mode 0600 on Unix.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Logs a complete (buffered) request body.
    pub fn log_request_body(&self, data: &[u8]) {
        self.log("request body", data);
    }

    /// Logs a single chunk of a (possibly streamed) response body.
    pub fn log_response_chunk(&self, data: &[u8]) {
        self.log("response body chunk", data);
    }

    fn log(&self, label: &str, data: &[u8]) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let Ok(mut file) = self.file.lock() else {
            return; // the lock was poisoned; drop the log entry
        };
        let _ = writeln!(
            file,
            "--- {} @{} ({} bytes) ---",
            label,
            timestamp,
            data.len()
        );
        let _ = file.write_all(data);
        let _ = writeln!(file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn new_logs_are_private_before_the_first_write() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir()?;
        let path = root.path().join("bodies.log");
        let _logger = BodyLogger::open(&path)?;
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        Ok(())
    }

    #[test]
    fn reopening_a_log_preserves_existing_entries() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("bodies.log");
        BodyLogger::open(&path)?.log_request_body(b"first request");
        let first = std::fs::read(&path)?;
        BodyLogger::open(&path)?.log_response_chunk(b"second response");
        let combined = std::fs::read(&path)?;
        assert!(combined.starts_with(&first));
        assert!(combined.ends_with(b"second response\n"));
        Ok(())
    }
}
