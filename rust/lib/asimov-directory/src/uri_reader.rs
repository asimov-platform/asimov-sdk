// This is free and unencumbered software released into the public domain.

use crate::UrisReader;
use std::io;
use tokio::io::{AsyncRead, AsyncSeek};

/// A single-IRI view of a [`UrisReader`], borrowing its next IRI once.
///
/// This supports reading either `.uri` or `.uris` input as one IRI. Buffering,
/// validation, and allocation behavior come from the wrapped reader. Buffered
/// I/O may read ahead, but trailing records are neither parsed nor validated.
///
/// ```
/// # async fn example() -> std::io::Result<()> {
/// use asimov_directory::{UriReader, UrisReader};
///
/// let uris = UrisReader::new(&b"urn:example:first\nurn:example:second\n"[..]);
/// let mut reader = UriReader::new(uris);
/// assert_eq!(reader.next_uri().await?, Some("urn:example:first"));
/// assert_eq!(reader.next_uri().await?, None);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct UriReader<R> {
    reader: UrisReader<R>,
    finished: bool,
}

impl<R> UriReader<R> {
    /// Wraps a reader at its current position, reusing its existing buffer.
    pub fn new(reader: UrisReader<R>) -> Self {
        Self {
            reader,
            finished: false,
        }
    }

    /// Recovers the wrapped reader, including any buffered trailing records.
    pub fn into_inner(self) -> UrisReader<R> {
        self.reader
    }
}

impl<R> From<UrisReader<R>> for UriReader<R> {
    fn from(reader: UrisReader<R>) -> Self {
        Self::new(reader)
    }
}

impl<R: AsyncRead + Unpin> UriReader<R> {
    /// Borrows the next IRI once, returning `None` on subsequent calls.
    ///
    /// Empty input returns `None`. Errors in the first record propagate and
    /// terminate iteration. Cancellation retains the partially read first record.
    pub async fn next_uri(&mut self) -> io::Result<Option<&str>> {
        if self.finished {
            return Ok(None);
        }
        let result = self.reader.next_uri().await;
        self.finished = true;
        result
    }
}

impl<R: AsyncSeek + Unpin> UriReader<R> {
    /// Rewinds the underlying input and permits one IRI to be read again.
    ///
    /// Cancellation and failure follow [`UrisReader::rewind`].
    pub async fn rewind(&mut self) -> io::Result<()> {
        self.finished = true;
        self.reader.rewind().await?;
        self.finished = false;
        Ok(())
    }
}
