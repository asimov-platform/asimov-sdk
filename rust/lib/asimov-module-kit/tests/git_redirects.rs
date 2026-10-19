// This is free and unencumbered software released into the public domain.

//! Regression probes for the embedded HTTP transport's origin boundary.
//!
//! Run the ignored probes when evaluating a libgit2 upgrade. A fixed proxy
//! selected for one origin is unsafe until both probes pass, or a per-request
//! transport hook lets the SDK resolve each destination independently.

#![cfg(feature = "module")]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::{format, vec::Vec};
use core::time::Duration;
use git2::{FetchOptions, ProxyOptions, RemoteRedirect, build::RepoBuilder};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Instant,
};

const DEADLINE: Duration = Duration::from_secs(5);

#[test]
fn no_redirect_blocks_a_different_host() {
    assert_redirect_stays_at_origin("http", "localhost", true);
}

#[test]
#[ignore = "libgit2 1.9.7 follows same-host port changes with RemoteRedirect::None"]
fn no_redirect_blocks_a_different_port() {
    assert_redirect_stays_at_origin("http", "127.0.0.1", true);
}

#[test]
#[ignore = "libgit2 1.9.7 follows HTTP-to-HTTPS changes with RemoteRedirect::None"]
fn no_redirect_blocks_a_different_scheme() {
    assert_redirect_stays_at_origin("https", "127.0.0.1", false);
}

fn assert_redirect_stays_at_origin(scheme: &str, host: &str, change_port: bool) {
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    source.set_nonblocking(true).unwrap();
    other.set_nonblocking(true).unwrap();
    let target = if change_port { &other } else { &source };
    let url = format!("http://{}/template", source.local_addr().unwrap());
    let location = format!(
        "{scheme}://{host}:{}/template/info/refs?service=git-upload-pack",
        target.local_addr().unwrap().port()
    );

    let clone = thread::spawn(move || {
        let scratch = tempfile::tempdir().unwrap();
        let mut fetch = FetchOptions::new();
        fetch
            .proxy_options(ProxyOptions::new())
            .follow_redirects(RemoteRedirect::None);
        RepoBuilder::new()
            .fetch_options(fetch)
            .clone(&url, scratch.path())
            .map(|_| ())
    });

    let start = Instant::now();
    let mut stream = loop {
        if let Some(stream) = accept(&source) {
            break stream;
        }
        assert!(
            !clone.is_finished(),
            "clone failed before reaching the fixture"
        );
        assert!(start.elapsed() < DEADLINE, "initial connection timed out");
        thread::sleep(Duration::from_millis(5));
    };
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.set_write_timeout(Some(DEADLINE)).unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        assert!(request.len() < 16 * 1024, "request headers are too large");
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        request.push(byte[0]);
    }
    assert!(request.starts_with(b"GET /template/info/refs?service=git-upload-pack "));
    write!(
        stream,
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    drop(stream);

    let mut contacted_target = false;
    loop {
        // Closing an unexpected connection also ends an attempted TLS handshake;
        // the scheme-change probe needs neither a certificate nor external TLS.
        if accept(target).is_some() {
            contacted_target = true;
        }
        if clone.is_finished() {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "redirect probe timed out");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(clone.join().unwrap().is_err());
    assert!(
        !contacted_target,
        "RemoteRedirect::None contacted another origin: {location}"
    );
}

fn accept(listener: &TcpListener) -> Option<TcpStream> {
    match listener.accept() {
        Ok((stream, _)) => Some(stream),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => None,
        Err(error) => panic!("fixture accept failed: {error}"),
    }
}
