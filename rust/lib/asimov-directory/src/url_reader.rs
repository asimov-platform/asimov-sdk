// This is free and unencumbered software released into the public domain.

use crate::UrlsReader;
use std::io;
use tokio::io::{AsyncRead, AsyncSeek};

/// A single-URL view of a [`UrlsReader`], borrowing its next URL once.
///
/// This supports reading either `.url` or `.urls` input as one URL. Buffering,
/// validation, and allocation behavior come from the wrapped reader. Buffered
/// I/O may read ahead, but trailing records are neither parsed nor validated.
///
/// ```
/// # async fn example() -> std::io::Result<()> {
/// use asimov_directory::{UrlReader, UrlsReader};
///
/// let urls = UrlsReader::new(&b"urn:example:first\nurn:example:second\n"[..]);
/// let mut reader = UrlReader::new(urls);
/// assert_eq!(reader.next_url().await?, Some("urn:example:first"));
/// assert_eq!(reader.next_url().await?, None);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct UrlReader<R> {
    reader: UrlsReader<R>,
    finished: bool,
}

impl<R> UrlReader<R> {
    /// Wraps a reader at its current position, reusing its existing buffer.
    pub fn new(reader: UrlsReader<R>) -> Self {
        Self {
            reader,
            finished: false,
        }
    }

    /// Recovers the wrapped reader, including any buffered trailing records.
    pub fn into_inner(self) -> UrlsReader<R> {
        self.reader
    }
}

impl<R> From<UrlsReader<R>> for UrlReader<R> {
    fn from(reader: UrlsReader<R>) -> Self {
        Self::new(reader)
    }
}

impl<R: AsyncRead + Unpin> UrlReader<R> {
    /// Borrows the next URL once, returning `None` on subsequent calls.
    ///
    /// Empty input returns `None`. Errors in the first record propagate and
    /// terminate iteration. Cancellation retains the partially read first record.
    pub async fn next_url(&mut self) -> io::Result<Option<&str>> {
        if self.finished {
            return Ok(None);
        }
        let result = self.reader.next_url().await;
        self.finished = true;
        result
    }
}

impl<R: AsyncSeek + Unpin> UrlReader<R> {
    /// Rewinds the underlying input and permits one URL to be read again.
    ///
    /// Cancellation and failure follow [`UrlsReader::rewind`].
    pub async fn rewind(&mut self) -> io::Result<()> {
        self.finished = true;
        self.reader.rewind().await?;
        self.finished = false;
        Ok(())
    }
}
