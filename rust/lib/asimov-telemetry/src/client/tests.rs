// This is free and unencumbered software released into the public domain.

use super::*;
use std::process::{Command, Output};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

const CHILD: &str = "ASIMOV_TELEMETRY_HTTP_TEST";
const DEADLINE: Duration = Duration::from_secs(15);
const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

// Isolate environment discovery without mutating a multithreaded test process.
fn child(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &format!("client::tests::{test}"), "--nocapture"])
        .env(CHILD, "1")
        .env_remove("REQUEST_METHOD")
        .env("NO_PROXY", "*")
        .env("no_proxy", "*");
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env(name, "http://127.0.0.1:1");
    }
    command
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

async fn receive(listener: &TcpListener, response: Option<&str>) -> String {
    timeout(DEADLINE, async {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = tokio::io::BufReader::new(stream);
        let mut request = String::new();
        while !request.ends_with("\r\n\r\n") {
            assert!(stream.read_line(&mut request).await.unwrap() > 0);
            assert!(request.len() < 8192);
        }
        let length = request
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        assert!(length < 1024 * 1024);
        let mut body = alloc::vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        request.push_str(core::str::from_utf8(&body).unwrap());
        if let Some(response) = response {
            stream
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
        }
        request
    })
    .await
    .expect("telemetry receiver timed out")
}

#[tokio::test]
async fn sends_installation_at_once_and_logs_of_exited_invocations() {
    if std::env::var_os(CHILD).is_none() {
        assert_success(
            child("sends_installation_at_once_and_logs_of_exited_invocations")
                .output()
                .unwrap(),
        );
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().to_path_buf();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let start = {
        let directory = directory.clone();
        move || Telemetry::start("client-test", "1", &directory, &endpoint)
    };
    let command = || Event::CommandStarted {
        command: "test".into(),
        modules: Vec::new(),
    };

    let previous = tokio::task::spawn_blocking(start.clone());
    let request = receive(&listener, Some(OK)).await;
    let id = fs::read_to_string(directory.join("id")).unwrap();
    assert!(request.contains("cli_installed"));
    assert!(request.contains(&id));
    let previous = previous.await.unwrap().unwrap();
    previous.log(command());
    previous.shutdown();
    drop(previous);
    fs::remove_file(directory.join("attempted")).unwrap();

    let current = start().unwrap();
    current.log(command());
    let request = receive(&listener, Some(OK)).await;
    timeout(Duration::from_secs(3), async {
        let logs = || {
            fs::read_dir(&directory)
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("events-"))
                .count()
        };
        while logs() > 1 || !directory.join("flushed").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    assert!(request.contains(&id));
    assert!(!request.contains("cli_installed"));
    assert_eq!(request.matches("cli_command_started").count(), 1);
    drop(current);
}

#[tokio::test]
async fn uploads_follow_shared_proxy_policy_and_preserve_failed_logs() {
    if std::env::var_os(CHILD).is_some() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events-previous.jsonl");
        let events: String = (0..=BATCH_SIZE)
            .map(|sequence| format!("{}\n", json!({"sequence": sequence})))
            .collect();
        fs::write(&path, &events).unwrap();
        let endpoint = std::env::var("ASIMOV_TELEMETRY_TEST_ENDPOINT").unwrap();
        let expected = std::env::var("ASIMOV_TELEMETRY_TEST_SUCCESS").unwrap() == "1";
        let directory_path = directory.path().to_path_buf();
        let result = tokio::task::spawn_blocking(move || {
            flush(
                &directory_path,
                HeaderValue::from_static("client-test"),
                &endpoint,
            )
        })
        .await
        .unwrap();
        assert_eq!(result.is_some(), expected);
        assert_eq!(path.exists(), !expected);
        assert_eq!(directory.path().join("flushed").exists(), expected);
        assert!(directory.path().join("attempted").exists());
        if !expected {
            assert_eq!(fs::read_to_string(path).unwrap(), events);
        }
        return;
    }

    for (mode, bypass, status) in [
        ("proxy", "", 200),
        ("host-bypass", "127.0.0.1", 200),
        ("wildcard-bypass", "*", 200),
        ("cgi", "", 200),
        ("redirect", "", 307),
        ("proxy-rejection", "", 407),
        ("broken-proxy", "", 0),
        ("invalid-origin", "", 0),
    ] {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = if mode == "invalid-origin" {
            "file:///telemetry".into()
        } else {
            format!("http://{}/v1/log_event", origin.local_addr().unwrap())
        };
        let proxied = !matches!(mode, "host-bypass" | "wildcard-bypass" | "cgi");
        let mut command = child("uploads_follow_shared_proxy_policy_and_preserve_failed_logs");
        command
            .env("ASIMOV_TELEMETRY_TEST_ENDPOINT", &endpoint)
            .env(
                "ASIMOV_TELEMETRY_TEST_SUCCESS",
                if status == 200 { "1" } else { "0" },
            )
            .env(
                "HTTP_PROXY",
                format!("http://user:pass@{}", proxy.local_addr().unwrap()),
            )
            .env("NO_PROXY", bypass);
        if mode == "cgi" {
            command.env("REQUEST_METHOD", "POST");
        }
        let task = tokio::task::spawn_blocking(move || command.output().unwrap());
        let selected = if proxied { &proxy } else { &origin };
        let response = format!(
            "HTTP/1.1 {status} Test\r\nlocation: {endpoint}/redirect\r\n\
             content-length: 0\r\nconnection: close\r\n\r\n"
        );
        let batches = if status == 200 { 2 } else { 1 };
        for batch in 0..batches {
            if mode == "invalid-origin" {
                break;
            }
            let request = receive(selected, (status != 0).then_some(response.as_str())).await;
            let (headers, body) = request.split_once("\r\n\r\n").unwrap();
            let target = if proxied { &endpoint } else { "/v1/log_event" };
            assert!(headers.starts_with(&format!("POST {target} HTTP/1.1\r\n")));
            assert!(headers.contains("statsig-api-key: client-test\r\n"));
            assert!(headers.contains("statsig-sdk-type: asimov.rs\r\n"));
            assert!(headers.contains(&format!(
                "statsig-sdk-version: {}\r\n",
                env!("CARGO_PKG_VERSION")
            )));
            assert!(headers.contains("statsig-client-time: "));
            assert_eq!(
                headers.contains("proxy-authorization: Basic dXNlcjpwYXNz"),
                proxied,
                "{mode}"
            );
            let body: Value = serde_json::from_str(body).unwrap();
            let events = body["events"].as_array().unwrap();
            assert_eq!(events.len(), if batch == 0 { BATCH_SIZE } else { 1 });
            assert_eq!(events[0]["sequence"], batch * BATCH_SIZE);
        }
        assert_success(timeout(DEADLINE, task).await.unwrap().unwrap());
        // No direct fallback, redirected upload, or unexpected extra batch.
        for listener in [&origin, &proxy] {
            assert!(
                timeout(Duration::from_millis(50), listener.accept())
                    .await
                    .is_err(),
                "unexpected connection: {mode}"
            );
        }
    }
}
