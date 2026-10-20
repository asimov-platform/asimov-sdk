// This is free and unencumbered software released into the public domain.

#![cfg(feature = "tokio")]

extern crate alloc;

use alloc::format;
use asimov_directory::UrlsReader;
use core::{
    future::Future,
    pin::{Pin, pin},
    task::{Context, Poll, Waker},
};
use std::io::{self, ErrorKind};
use tokio::io::{AsyncRead, ReadBuf};

/// Splits reads inside both UTF-8 sequences and line delimiters, yielding
/// between fragments so partially read lines survive suspension/cancellation.
struct Fragmented<'a> {
    bytes: &'a [u8],
    chunk: usize,
    pending: bool,
}

impl AsyncRead for Fragmented<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pending {
            self.pending = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let count = self.bytes.len().min(self.chunk).min(buf.remaining());
        buf.put_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        self.pending = true;
        Poll::Ready(Ok(()))
    }
}

fn ready<F: Future>(future: F) -> F::Output {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("in-memory read unexpectedly suspended"),
    }
}

#[tokio::test]
async fn fragments_and_buffer_boundaries_preserve_url_bytes() -> io::Result<()> {
    let urls = [
        "HTTPS://Example.COM/%2f?q=%41#fragment",
        "https://例え.テスト/🦀",
        "urn:isbn:9780140328721",
        "urn:isbn:9780140328721",
    ];
    let input = format!("{}\n", urls.join("\n"));
    for capacity in [1, 7, 64] {
        for chunk in [1, 2, 7, 31, 64] {
            let input = Fragmented {
                bytes: input.as_bytes(),
                chunk,
                pending: false,
            };
            let mut reader = UrlsReader::with_capacity(capacity, input);
            for url in urls {
                assert_eq!(reader.next_url().await?, Some(url));
            }
            assert_eq!(reader.next_url().await?, None);
            assert_eq!(reader.next_url().await?, None);
        }
    }
    Ok(())
}

#[test]
fn reading_many_urls_performs_no_per_line_allocations() {
    let url = "https://例え.テスト/%2f?x=%41#fragment";
    let input = format!("{url}\n").repeat(20_000);
    let mut reader = UrlsReader::with_capacity(128, input.as_bytes());
    let mut count = 0;
    let allocations = allocation_counter::measure(|| {
        while let Some(line) = ready(reader.next_url()).unwrap() {
            assert_eq!(line, url);
            count += 1;
        }
    });
    assert_eq!(count, 20_000);
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}

#[test]
fn long_lines_grow_the_buffer_once_then_reuse_it() {
    let long = format!("https://example.com/{}", "x".repeat(128 * 1024));
    let input = format!("{long}\nurn:short\n").repeat(8);
    let mut reader = UrlsReader::with_capacity(8, input.as_bytes());
    assert_eq!(ready(reader.next_url()).unwrap(), Some(long.as_str()));
    let allocations = allocation_counter::measure(|| {
        for index in 0..15 {
            let expected = if index % 2 == 0 { "urn:short" } else { &long };
            assert_eq!(ready(reader.next_url()).unwrap(), Some(expected));
        }
        assert_eq!(ready(reader.next_url()).unwrap(), None);
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}

#[tokio::test]
async fn malformed_lines_fail_after_prior_output_and_stop_iteration() -> io::Result<()> {
    for invalid in [
        b"\n".as_slice(),
        b"relative/path\n",
        b"https://example.com\r\n",
        b"https://example.com/\xff\n",
        b"https://example.com",
        b"https://example.com/%zz\n",
    ] {
        let mut input = b"https://valid.example\n".to_vec();
        input.extend_from_slice(invalid);
        let mut reader = UrlsReader::with_capacity(4, input.as_slice());
        assert_eq!(reader.next_url().await?, Some("https://valid.example"));
        assert_eq!(
            reader.next_url().await.unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(reader.next_url().await?, None);
    }
    let mut empty = UrlsReader::<&[u8]>::empty();
    assert_eq!(empty.next_url().await?, None);
    let mut empty_file = UrlsReader::new(b"".as_slice());
    assert_eq!(empty_file.next_url().await?, None);
    Ok(())
}

#[tokio::test]
async fn cancellation_retains_a_partially_read_line() -> io::Result<()> {
    let input = Fragmented {
        bytes: b"https://first.example\nhttps://second.example\n",
        chunk: 3,
        pending: false,
    };
    let mut reader = UrlsReader::with_capacity(4, input);
    {
        let next = pin!(reader.next_url());
        assert!(
            next.poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }
    assert_eq!(reader.next_url().await?, Some("https://first.example"));
    assert_eq!(reader.next_url().await?, Some("https://second.example"));
    assert_eq!(reader.next_url().await?, None);
    Ok(())
}

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
async fn io_errors_are_preserved_and_rewind_restarts_iteration() -> io::Result<()> {
    let mut reader = UrlsReader::new(Failing(b"https://example.com\nhttps://partial"));
    assert_eq!(reader.next_url().await?, Some("https://example.com"));
    assert_eq!(
        reader.next_url().await.unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(reader.next_url().await?, None);

    let mut reader = UrlsReader::new(io::Cursor::new(b"https://example.com\n"));
    for _ in 0..2 {
        assert_eq!(reader.next_url().await?, Some("https://example.com"));
        assert_eq!(reader.next_url().await?, None);
        reader.rewind().await?;
    }
    Ok(())
}
