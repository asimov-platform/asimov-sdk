// This is free and unencumbered software released into the public domain.

#![cfg(feature = "tokio")]

use asimov_directory::{UrlReader, UrlsReader};
use core::{
    future::Future,
    pin::{Pin, pin},
    task::{Context, Poll, Waker},
};
use std::io::{self, ErrorKind};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};

fn ready<F: Future>(future: F) -> F::Output {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("in-memory read unexpectedly suspended"),
    }
}

#[tokio::test]
async fn trailing_records_are_ignored_even_when_malformed() -> io::Result<()> {
    for input in [
        b"urn:first\nurn:second\n".as_slice(),
        b"urn:first\n\xff\n",
        b"urn:first\nrelative/path\n",
        b"urn:first\nunterminated",
        b"urn:first\n\n",
    ] {
        let mut reader = UrlReader::new(UrlsReader::new(input));
        assert_eq!(reader.next_url().await?, Some("urn:first"));
        assert_eq!(reader.next_url().await?, None);
        assert_eq!(reader.next_url().await?, None);
    }
    Ok(())
}

#[test]
fn wrapping_preserves_position_and_buffered_remainder_without_allocating() {
    let mut urls = UrlsReader::new(b"urn:first\nurn:second\nurn:third\n".as_slice());
    assert_eq!(ready(urls.next_url()).unwrap(), Some("urn:first"));
    let allocations = allocation_counter::measure(|| {
        let mut reader = UrlReader::from(urls);
        assert_eq!(ready(reader.next_url()).unwrap(), Some("urn:second"));
        assert_eq!(ready(reader.next_url()).unwrap(), None);
        let mut urls = reader.into_inner();
        assert_eq!(ready(urls.next_url()).unwrap(), Some("urn:third"));
        assert_eq!(ready(urls.next_url()).unwrap(), None);
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}

#[tokio::test]
async fn empty_input_and_first_record_errors_follow_the_wrapped_reader() -> io::Result<()> {
    for input in [
        b"".as_slice(),
        b"\n",
        b"relative/path\nurn:valid\n",
        b"\xff\n",
        b"urn:unterminated",
    ] {
        let mut reader = UrlReader::new(UrlsReader::new(input));
        if input.is_empty() {
            assert_eq!(reader.next_url().await?, None);
        } else {
            assert_eq!(
                reader.next_url().await.unwrap_err().kind(),
                ErrorKind::InvalidData
            );
        }
        assert_eq!(reader.next_url().await?, None);
    }
    let mut reader = UrlReader::from(UrlsReader::<&[u8]>::empty());
    assert_eq!(reader.next_url().await?, None);
    Ok(())
}

/// Provides a complete first line, then reports an error on any further read.
struct Failing<'a>(&'a [u8]);

impl AsyncRead for Failing<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.0.is_empty() {
            return Poll::Ready(Err(ErrorKind::PermissionDenied.into()));
        }
        let count = self.0.len().min(buf.remaining());
        buf.put_slice(&self.0[..count]);
        self.0 = &self.0[count..];
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn io_errors_propagate_only_when_reading_the_first_url() -> io::Result<()> {
    let mut reader = UrlReader::new(UrlsReader::new(Failing(b"")));
    assert_eq!(
        reader.next_url().await.unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(reader.next_url().await?, None);

    let mut reader = UrlReader::new(UrlsReader::new(Failing(b"urn:first\n")));
    assert_eq!(reader.next_url().await?, Some("urn:first"));
    assert_eq!(reader.next_url().await?, None);
    assert_eq!(
        reader.into_inner().next_url().await.unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_does_not_consume_the_one_url_allowance() -> io::Result<()> {
    let (mut writer, input) = tokio::io::duplex(64);
    let mut reader = UrlReader::new(UrlsReader::with_capacity(4, input));
    writer.write_all(b"urn:").await?;
    {
        let next = pin!(reader.next_url());
        assert!(
            next.poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    writer.write_all(b"first\nurn:second\n").await?;
    assert_eq!(reader.next_url().await?, Some("urn:first"));
    assert_eq!(reader.next_url().await?, None);
    Ok(())
}

#[tokio::test]
async fn rewind_permits_one_url_again() -> io::Result<()> {
    let input = io::Cursor::new(b"urn:first\nurn:second\n");
    let mut reader = UrlReader::new(UrlsReader::new(input));
    for _ in 0..2 {
        assert_eq!(reader.next_url().await?, Some("urn:first"));
        assert_eq!(reader.next_url().await?, None);
        reader.rewind().await?;
    }
    Ok(())
}
