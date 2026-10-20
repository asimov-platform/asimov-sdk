// This is free and unencumbered software released into the public domain.

use crate::{InvalidUrlFile, url_file::validate_url};
use alloc::{vec, vec::Vec};
use std::io::{self, ErrorKind};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};

/// An asynchronous `.urls` reader yielding borrowed, newline-free URL slices.
///
/// A single reusable buffer holds input bytes. Reading and validating a URL
/// allocates nothing once that buffer can hold the longest line encountered;
/// longer lines grow it as needed. Memory usage is independent of file size.
/// Allocations performed by the underlying I/O implementation are separate.
///
/// Each slice borrows the reader until the next call, so this is a lending
/// reader rather than a `Stream` of owned strings. Order, spelling, and duplicate
/// URLs are preserved. Validation follows [`crate::UrlFile`].
///
/// ```
/// # async fn example() -> std::io::Result<()> {
/// use asimov_directory::UrlsReader;
///
/// let mut reader = UrlsReader::new(&b"urn:example:one\nurn:example:two\n"[..]);
/// while let Some(url) = reader.next_url().await? {
///     assert!(url.starts_with("urn:example:"));
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct UrlsReader<R> {
    reader: Option<R>,
    buffer: Vec<u8>,
    start: usize,
    end: usize,
    scanned: usize,
    finished: bool,
}

impl<R> UrlsReader<R> {
    /// Creates a reader with an initial 8 KiB buffer.
    pub fn new(reader: R) -> Self {
        Self::with_capacity(8192, reader)
    }

    /// Creates a reader with the given initial buffer size (at least one byte).
    ///
    /// This is an initial allocation size, not a maximum URL length.
    pub fn with_capacity(capacity: usize, reader: R) -> Self {
        Self {
            reader: Some(reader),
            buffer: vec![0; capacity.max(1)],
            start: 0,
            end: 0,
            scanned: 0,
            finished: false,
        }
    }

    /// Creates an empty reader without allocating or accessing a file.
    pub fn empty() -> Self {
        Self {
            reader: None,
            buffer: Vec::new(),
            start: 0,
            end: 0,
            scanned: 0,
            finished: true,
        }
    }
}

impl<R: AsyncRead + Unpin> UrlsReader<R> {
    /// Reads the next URL, borrowing its bytes from the reusable input buffer.
    ///
    /// Returns `None` at EOF, including for an empty file. Invalid UTF-8, invalid
    /// URLs, blank lines, CRLF, and a missing final LF produce `InvalidData`.
    /// Errors terminate iteration; earlier URLs may already have been emitted.
    /// Cancelling this future does not discard partially read lines.
    pub async fn next_url(&mut self) -> io::Result<Option<&str>> {
        if self.finished {
            return Ok(None);
        }

        let newline = loop {
            if let Some(offset) = memchr::memchr(b'\n', &self.buffer[self.scanned..self.end]) {
                break self.scanned + offset;
            }
            self.scanned = self.end;
            if self.start != 0 {
                self.buffer.copy_within(self.start..self.end, 0);
                self.end -= self.start;
                self.scanned -= self.start;
                self.start = 0;
            }
            if self.end == self.buffer.len() {
                self.buffer.resize(self.buffer.len() * 2, 0);
            }
            match self
                .reader
                .as_mut()
                .expect("unfinished reader has input")
                .read(&mut self.buffer[self.end..])
                .await
            {
                Ok(0) => {
                    self.finished = true;
                    return if self.end == 0 {
                        Ok(None)
                    } else {
                        Err(io::Error::new(ErrorKind::InvalidData, InvalidUrlFile))
                    };
                },
                Ok(count) => self.end += count,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.finished = true;
                    return Err(error);
                },
            }
        };

        let start = self.start;
        self.start = newline + 1;
        self.scanned = self.start;
        let line = core::str::from_utf8(&self.buffer[start..newline])
            .map_err(|_| InvalidUrlFile)
            .and_then(|line| validate_url(line).map(|()| line));
        match line {
            Ok(line) => Ok(Some(line)),
            Err(error) => {
                self.finished = true;
                Err(io::Error::new(ErrorKind::InvalidData, error))
            },
        }
    }
}

impl<R: AsyncSeek + Unpin> UrlsReader<R> {
    /// Rewinds the input for another pass, retaining the allocated buffer.
    ///
    /// On cancellation or failure, iteration stays stopped until a successful
    /// rewind. Empty readers remain empty.
    pub async fn rewind(&mut self) -> io::Result<()> {
        self.finished = true;
        if let Some(reader) = &mut self.reader {
            reader.rewind().await?;
            self.start = 0;
            self.end = 0;
            self.scanned = 0;
            self.finished = false;
        }
        Ok(())
    }
}
