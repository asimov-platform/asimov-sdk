// This is free and unencumbered software released into the public domain.

use alloc::{format, string::String};
use core::time::Duration;
use std::process::{Command, Output};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

pub const CHILD: &str = "ASIMOV_INSTALLER_HTTP_TEST";
pub const DEADLINE: Duration = Duration::from_secs(15);

pub fn child(test: &str, case: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &format!("installer::{test}"), "--nocapture"])
        .env(CHILD, case)
        .env_remove("REQUEST_METHOD")
        .env("no_proxy", "*")
        .env("NO_PROXY", "*");
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

pub fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub async fn request(stream: &mut TcpStream) -> String {
    timeout(DEADLINE, async {
        let mut stream = tokio::io::BufReader::new(stream);
        let mut request = String::new();
        while !request.ends_with("\r\n\r\n") {
            assert!(stream.read_line(&mut request).await.unwrap() > 0);
            assert!(request.len() < 8192);
        }
        request
    })
    .await
    .expect("installer request timed out")
}

pub async fn receive(listener: &TcpListener, response: &str) -> String {
    timeout(DEADLINE, async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = request(&mut stream).await;
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    })
    .await
    .expect("installer receiver timed out")
}

pub fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

pub async fn assert_idle(listener: &TcpListener) {
    assert!(
        timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "unexpected connection"
    );
}
