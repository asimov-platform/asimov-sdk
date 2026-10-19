// This is free and unencumbered software released into the public domain.

#![cfg(feature = "std")]

extern crate alloc;

use alloc::string::String;
use asimov_huggingface::{HuggingfaceError, ensure_file, ensure_snapshot, file_exists};
use core::time::Duration;
use hf_hub::{Cache, Repo};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const CHILD: &str = "ASIMOV_HF_HTTP_TEST";
const DEADLINE: Duration = Duration::from_secs(15);
const COMMIT: &str = "1111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222";
const ETAG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn child(test: &str, case: &str, home: &Path, endpoint: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, case)
        .env("HF_HOME", home)
        .env("HF_ENDPOINT", endpoint)
        .env_remove("REQUEST_METHOD")
        .env("no_proxy", "*")
        .env("NO_PROXY", "*");
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env(key, "http://127.0.0.1:1");
    }
    command
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn accept(listener: &TcpListener) -> (TcpStream, String) {
    timeout(DEADLINE, async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut stream);
        let mut request = String::new();
        while !request.ends_with("\r\n\r\n") {
            assert!(reader.read_line(&mut request).await.unwrap() > 0);
            assert!(request.len() < 16384);
        }
        drop(reader);
        (stream, request)
    })
    .await
    .expect("HTTP fixture timed out")
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

async fn reply(stream: &mut TcpStream, status: u16, headers: &str, body: &str) {
    stream.write_all(format!(
        "HTTP/1.1 {status} Test\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).as_bytes()).await.unwrap();
}

fn ids(commit: &str) -> String {
    format!("x-repo-commit: {commit}\r\nx-linked-etag: \"{ETAG}\"\r\n")
}

async fn probe(listener: &TcpListener, commit: &str, size: u64) -> String {
    let (mut stream, request) = accept(listener).await;
    assert_eq!(header(&request, "range"), Some("bytes=0-0"));
    let range = if size == 0 {
        "bytes */0".into()
    } else {
        format!("bytes 0-0/{size}")
    };
    reply(
        &mut stream,
        if size == 0 { 416 } else { 206 },
        &format!("{}Content-Range: {range}\r\n", ids(commit)),
        if size == 0 { "" } else { "h" },
    )
    .await;
    request
}

async fn idle(listener: &TcpListener) {
    assert!(
        timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

async fn file_contains(path: &Path, bytes: &[u8]) {
    timeout(DEADLINE, async {
        while !fs::read(path).is_ok_and(|content| content == bytes) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("download did not stream to its partial file");
}

fn seed_part(home: &Path, content: &[u8]) {
    let directory = home.join("hub/models--org--model/blobs");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(format!("{ETAG}.part")), content).unwrap();
}

#[tokio::test]
async fn proxy_redirects_scope_tokens_resume_and_reuse_cache() {
    if std::env::var_os(CHILD).is_some() {
        // Invoke the synchronous API inside a runtime, as the installer does.
        let path = ensure_file("org/model", "weights.bin").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        assert_eq!(file_exists("org/model", "weights.bin"), Some(path.clone()));
        assert_eq!(
            Cache::from_env()
                .repo(Repo::model("org/model".into()))
                .get("weights.bin"),
            Some(path.clone())
        );
        assert_eq!(ensure_file("org/model", "weights.bin").unwrap(), path);
        return;
    }
    let home = tempfile::tempdir().unwrap();
    fs::write(home.path().join("token"), "hub-token\n").unwrap();
    seed_part(home.path(), b"he");
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let storage = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = "http://hub.invalid/prefix";
    let mut command = child(
        "proxy_redirects_scope_tokens_resume_and_reuse_cache",
        "download",
        home.path(),
        endpoint,
    );
    command
        .env(
            "HTTP_PROXY",
            format!("http://user:pass@{}", proxy.local_addr().unwrap()),
        )
        .env("NO_PROXY", "127.0.0.1");
    let task = tokio::task::spawn_blocking(move || command.output().unwrap());

    let (mut stream, request) = accept(&proxy).await;
    assert!(request.starts_with(
        "GET http://hub.invalid/prefix/org/model/resolve/main/weights.bin HTTP/1.1\r\n"
    ));
    assert_eq!(header(&request, "authorization"), Some("Bearer hub-token"));
    reply(
        &mut stream,
        307,
        "Location: /prefix/renamed/weights.bin\r\n",
        "",
    )
    .await;
    let (mut stream, request) = accept(&proxy).await;
    assert!(request.starts_with("GET http://hub.invalid/prefix/renamed/weights.bin HTTP/1.1\r\n"));
    assert_eq!(header(&request, "authorization"), Some("Bearer hub-token"));
    assert_eq!(header(&request, "range"), Some("bytes=0-0"));
    reply(
        &mut stream,
        302,
        &format!(
            "{}Location: http://{}/metadata\r\n",
            ids(COMMIT),
            storage.local_addr().unwrap()
        ),
        "",
    )
    .await;
    let (mut stream, request) = accept(&storage).await;
    assert_eq!(header(&request, "authorization"), None);
    assert_eq!(header(&request, "proxy-authorization"), None);
    // A storage provider's commit header must not override the Hub's identity.
    reply(
        &mut stream,
        206,
        &format!("{}Content-Range: bytes 0-0/5\r\n", ids(OTHER)),
        "h",
    )
    .await;

    let (mut stream, request) = accept(&proxy).await;
    assert!(request.starts_with(&format!(
        "GET http://hub.invalid/prefix/org/model/resolve/{COMMIT}/weights.bin HTTP/1.1\r\n"
    )));
    assert_eq!(header(&request, "range"), Some("bytes=2-"));
    assert_eq!(header(&request, "authorization"), Some("Bearer hub-token"));
    assert_eq!(
        header(&request, "proxy-authorization"),
        Some("Basic dXNlcjpwYXNz")
    );
    reply(
        &mut stream,
        302,
        &format!(
            "Location: http://{}/file?token=a%2Fb\r\n",
            storage.local_addr().unwrap()
        ),
        "",
    )
    .await;
    let (mut stream, request) = accept(&storage).await;
    assert!(request.starts_with("GET /file?token=a%2Fb HTTP/1.1\r\n"));
    assert_eq!(header(&request, "range"), Some("bytes=2-"));
    assert_eq!(header(&request, "accept-encoding"), Some("identity"));
    assert_eq!(header(&request, "authorization"), None);
    assert_eq!(header(&request, "proxy-authorization"), None);
    stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-4/5\r\nContent-Length: 3\r\nConnection: close\r\n\r\nl").await.unwrap();
    let root = home.path().join("hub/models--org--model");
    file_contains(&root.join(format!("blobs/{ETAG}.part")), b"hel").await;
    assert!(!root.join("refs/main").exists());
    assert!(
        !root
            .join(format!("snapshots/{COMMIT}/weights.bin"))
            .exists()
    );
    stream.write_all(b"lo").await.unwrap();
    assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
    assert!(!root.join(format!("blobs/{ETAG}.part")).exists());

    // Cache hits must work without even parsing network configuration.
    assert_success(
        child(
            "proxy_redirects_scope_tokens_resume_and_reuse_cache",
            "cached",
            home.path(),
            "not a URL",
        )
        .output()
        .unwrap(),
    );
    idle(&proxy).await;
    idle(&storage).await;
}

#[tokio::test]
async fn failed_and_ignored_ranges_do_not_corrupt_the_cache() {
    if let Ok(case) = std::env::var(CHILD) {
        let result = ensure_file("org/model", "weights.bin");
        match case.as_str() {
            "truncated" => assert!(matches!(result, Err(HuggingfaceError::Http(_)))),
            "wrong-range" | "oversized" | "encoded" => {
                assert!(matches!(result, Err(HuggingfaceError::InvalidResponse(_))))
            },
            "empty" => assert_eq!(fs::read(result.unwrap()).unwrap(), b""),
            _ => assert_eq!(fs::read(result.unwrap()).unwrap(), b"hello"),
        }
        return;
    }
    for case in [
        "truncated",
        "wrong-range",
        "oversized",
        "encoded",
        "ignored-range",
        "empty",
    ] {
        let home = tempfile::tempdir().unwrap();
        seed_part(
            home.path(),
            if case == "empty" || case == "oversized" {
                b""
            } else {
                b"he"
            },
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let mut command = child(
            "failed_and_ignored_ranges_do_not_corrupt_the_cache",
            case,
            home.path(),
            &endpoint,
        );
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        probe(&listener, COMMIT, if case == "empty" { 0 } else { 5 }).await;
        let root = home.path().join("hub/models--org--model");
        let part = root.join(format!("blobs/{ETAG}.part"));
        if case != "empty" {
            let (mut stream, request) = accept(&listener).await;
            assert_eq!(
                header(&request, "range"),
                Some(if case == "oversized" {
                    "bytes=0-"
                } else {
                    "bytes=2-"
                })
            );
            match case {
                "truncated" => {
                    stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-4/5\r\nContent-Length: 3\r\n\r\nl").await.unwrap();
                    file_contains(&part, b"hel").await;
                },
                "wrong-range" => {
                    reply(&mut stream, 206, "Content-Range: bytes 0-4/5\r\n", "hello").await
                },
                "oversized" => reply(&mut stream, 200, "", "hello!").await,
                "encoded" => reply(&mut stream, 200, "Content-Encoding: gzip\r\n", "hello").await,
                _ => reply(&mut stream, 200, "", "hello").await,
            }
        }
        assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        if matches!(case, "truncated" | "wrong-range" | "oversized" | "encoded") {
            assert!(!root.join("refs/main").exists());
            assert!(!root.join(format!("blobs/{ETAG}")).exists());
            assert!(
                !root
                    .join(format!("snapshots/{COMMIT}/weights.bin"))
                    .exists()
            );
            assert_eq!(
                fs::read(&part).unwrap(),
                match case {
                    "truncated" => b"hel".as_slice(),
                    "wrong-range" | "encoded" => b"he",
                    _ => b"",
                }
            );
        }
        if case == "truncated" {
            let mut command = child(
                "failed_and_ignored_ranges_do_not_corrupt_the_cache",
                "retry",
                home.path(),
                &endpoint,
            );
            let task = tokio::task::spawn_blocking(move || command.output().unwrap());
            probe(&listener, COMMIT, 5).await;
            let (mut stream, request) = accept(&listener).await;
            assert_eq!(header(&request, "range"), Some("bytes=3-"));
            reply(&mut stream, 206, "Content-Range: bytes 3-4/5\r\n", "lo").await;
            assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        }
        idle(&listener).await;
    }
}

#[tokio::test]
async fn snapshots_use_one_commit_and_return_the_snapshot_root() {
    if let Ok(case) = std::env::var(CHILD) {
        let result = ensure_snapshot("org/model", Some("refs/pr/2"));
        if case == "changed-commit" {
            assert!(matches!(result, Err(HuggingfaceError::InvalidResponse(_))));
        } else if case == "bad-filename" {
            assert!(matches!(result, Err(HuggingfaceError::InvalidInput(_))));
        } else if case == "empty-snapshot" {
            assert!(matches!(result, Err(HuggingfaceError::EmptySnapshot)));
        } else {
            let root = result.unwrap();
            assert_eq!(root.file_name().unwrap(), OTHER);
            assert_eq!(
                fs::read(root.join("nested/weights.bin")).unwrap(),
                b"newdata"
            );
            assert_eq!(fs::read(root.join("cached.txt")).unwrap(), b"cached");
            let repo = Repo::with_revision(
                "org/model".into(),
                hf_hub::RepoType::Model,
                "refs/pr/2".into(),
            );
            assert_eq!(
                Cache::from_env().repo(repo).get("nested/weights.bin"),
                Some(root.join("nested/weights.bin"))
            );
        }
        return;
    }
    for case in [
        "snapshot",
        "changed-commit",
        "bad-filename",
        "empty-snapshot",
    ] {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("hub/models--org--model");
        fs::create_dir_all(root.join(format!("snapshots/{COMMIT}/nested"))).unwrap();
        fs::write(
            root.join(format!("snapshots/{COMMIT}/nested/weights.bin")),
            "old",
        )
        .unwrap();
        fs::create_dir_all(root.join(format!("snapshots/{OTHER}"))).unwrap();
        fs::write(root.join(format!("snapshots/{OTHER}/cached.txt")), "cached").unwrap();
        fs::create_dir_all(root.join("refs")).unwrap();
        fs::write(root.join("refs/main"), COMMIT).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/prefix", listener.local_addr().unwrap());
        let mut command = child(
            "snapshots_use_one_commit_and_return_the_snapshot_root",
            case,
            home.path(),
            &endpoint,
        );
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let (mut stream, request) = accept(&listener).await;
        assert!(
            request.starts_with(
                "GET /prefix/api/models/org/model/revision/refs%2Fpr%2F2 HTTP/1.1\r\n"
            )
        );
        let siblings = match case {
            "bad-filename" => r#"[{"rfilename":"../escape"}]"#,
            "empty-snapshot" => "[]",
            _ => r#"[{"rfilename":"nested/weights.bin"},{"rfilename":"cached.txt"}]"#,
        };
        reply(
            &mut stream,
            200,
            "Content-Type: application/json\r\n",
            &format!(r#"{{"sha":"{OTHER}","siblings":{siblings}}}"#),
        )
        .await;
        if matches!(case, "bad-filename" | "empty-snapshot") {
            assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
            assert!(!root.join("refs/refs/pr/2").exists());
            assert!(!root.join("snapshots/escape").exists());
            idle(&listener).await;
            continue;
        }
        let request = probe(
            &listener,
            if case == "changed-commit" {
                COMMIT
            } else {
                OTHER
            },
            7,
        )
        .await;
        assert!(request.starts_with(&format!(
            "GET /prefix/org/model/resolve/{OTHER}/nested/weights.bin HTTP/1.1\r\n"
        )));
        if case == "snapshot" {
            let (mut stream, request) = accept(&listener).await;
            assert!(request.starts_with(&format!(
                "GET /prefix/org/model/resolve/{OTHER}/nested/weights.bin HTTP/1.1\r\n"
            )));
            reply(
                &mut stream,
                206,
                "Content-Range: bytes 0-6/7\r\n",
                "newdata",
            )
            .await;
        }
        assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        assert_eq!(fs::read_to_string(root.join("refs/main")).unwrap(), COMMIT);
        if case == "changed-commit" {
            assert!(!root.join("refs/refs/pr/2").exists());
        } else {
            assert_eq!(
                fs::read_to_string(root.join("refs/refs/pr/2")).unwrap(),
                OTHER
            );
        }
        idle(&listener).await;
    }
}

#[tokio::test]
async fn concurrent_downloads_share_the_completed_blob() {
    if std::env::var_os(CHILD).is_some() {
        std::thread::scope(|scope| {
            let first = scope.spawn(|| ensure_file("org/model", "weights.bin").unwrap());
            let second = scope.spawn(|| ensure_file("org/model", "weights.bin").unwrap());
            let first = first.join().unwrap();
            assert_eq!(second.join().unwrap(), first);
            assert_eq!(fs::read(first).unwrap(), b"hello");
        });
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut command = child(
        "concurrent_downloads_share_the_completed_blob",
        "1",
        home.path(),
        &endpoint,
    );
    let task = tokio::task::spawn_blocking(move || command.output().unwrap());
    let mut probes = 0;
    let mut body = None;
    for _ in 0..3 {
        let (mut stream, request) = accept(&listener).await;
        if header(&request, "range") == Some("bytes=0-0") {
            probes += 1;
            reply(
                &mut stream,
                206,
                &format!("{}Content-Range: bytes 0-0/5\r\n", ids(COMMIT)),
                "h",
            )
            .await;
        } else {
            assert_eq!(header(&request, "range"), Some("bytes=0-"));
            assert!(body.is_none());
            body = Some(stream);
        }
    }
    assert_eq!(probes, 2);
    reply(
        &mut body.unwrap(),
        206,
        "Content-Range: bytes 0-4/5\r\n",
        "hello",
    )
    .await;
    assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
    idle(&listener).await;
}

#[tokio::test]
async fn proxy_redirect_and_metadata_failures_are_reported() {
    if let Ok(case) = std::env::var(CHILD) {
        let result = ensure_file("org/model", "weights.bin");
        match case.as_str() {
            "proxy" => assert!(
                matches!(result, Err(HuggingfaceError::Http(error)) if error.status() == Some(reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED))
            ),
            "origin" => assert!(matches!(result, Err(HuggingfaceError::Proxy(_)))),
            _ => assert!(matches!(result, Err(HuggingfaceError::InvalidResponse(_)))),
        }
        return;
    }
    for case in ["proxy", "origin", "loop", "metadata"] {
        let home = tempfile::tempdir().unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut command = child(
            "proxy_redirect_and_metadata_failures_are_reported",
            case,
            home.path(),
            "http://hub.invalid",
        );
        command
            .env(
                "HTTP_PROXY",
                format!("http://{}", proxy.local_addr().unwrap()),
            )
            .env("NO_PROXY", "");
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        for _ in 0..if case == "loop" { 11 } else { 1 } {
            let (mut stream, _) = accept(&proxy).await;
            match case {
                "proxy" => reply(&mut stream, 407, "", "").await,
                "origin" => reply(&mut stream, 302, "Location: ftp://invalid/file\r\n", "").await,
                "loop" => reply(&mut stream, 302, "Location: /loop\r\n", "").await,
                _ => reply(&mut stream, 206, "x-repo-commit: ../escape\r\nx-linked-etag: ../escape\r\nContent-Range: bytes 0-0/5\r\n", "h").await,
            }
        }
        assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        assert!(!home.path().join("hub/models--org--model").exists());
        idle(&proxy).await;
    }
}
