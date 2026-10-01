// This is free and unencumbered software released into the public domain.

use crate::{Event, Outcome};
use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::time::Duration;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::Path,
    sync::{Mutex, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

const ENDPOINT: &str = "https://api.statsig.com/v1/log_event";
const FLUSH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);
const STALE_SHUTDOWN_WAIT: Duration = Duration::from_millis(500);
const MAX_EVENT_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const BATCH_SIZE: usize = 500;

pub struct Telemetry {
    events: Mutex<File>,
    user: User,
    invocation_id: String,
    caller: Caller,
    stale_flush: Mutex<Option<mpsc::Receiver<()>>>,
}

impl Telemetry {
    /// Events are appended to a per-invocation log in `directory`, which stays locked while
    /// the process lives. Logs of exited processes are sent at most every [`FLUSH_INTERVAL`].
    pub fn new(
        client_key: &str,
        user_id: String,
        app_version: &str,
        directory: &Path,
    ) -> Option<Self> {
        Self::start(client_key, user_id, app_version, directory, ENDPOINT)
    }

    fn start(
        client_key: &str,
        user_id: String,
        app_version: &str,
        directory: &Path,
        endpoint: &str,
    ) -> Option<Self> {
        if !client_key.starts_with("client-") || client_key.len() <= "client-".len() {
            return None;
        }
        let mut key = HeaderValue::from_str(client_key).ok()?;
        key.set_sensitive(true);
        fs::create_dir_all(directory).ok()?;
        let invocation_id = uuid::Uuid::now_v7().to_string();
        let events =
            File::create_new(directory.join(format!("events-{invocation_id}.jsonl"))).ok()?;
        events.try_lock().ok()?;

        let age = |name: &str| {
            fs::metadata(directory.join(name))
                .and_then(|metadata| metadata.modified())
                .map(|time| SystemTime::now().duration_since(time).unwrap_or_default())
                .unwrap_or(Duration::MAX)
        };
        let flush = (age("attempted") >= FLUSH_INTERVAL)
            .then(|| {
                let (sender, receiver) = mpsc::channel();
                let directory = directory.to_path_buf();
                let endpoint = endpoint.to_string();
                std::thread::Builder::new()
                    .name("asimov-telemetry".into())
                    .spawn(move || {
                        flush(&directory, key, &endpoint);
                        let _ = sender.send(());
                    })
                    .ok()
                    .map(|_| receiver)
            })
            .flatten();

        Some(Self {
            events: Mutex::new(events),
            user: User {
                user_id,
                app_version: app_version.to_string(),
                custom: BTreeMap::from([
                    ("target_os", std::env::consts::OS),
                    ("target_arch", std::env::consts::ARCH),
                ]),
            },
            invocation_id,
            caller: Caller::detect(),
            stale_flush: Mutex::new(flush.filter(|_| age("flushed") >= STALE_AFTER)),
        })
    }

    pub fn log(&self, event: Event) {
        let event = wire_event(
            event,
            timestamp(),
            &self.user,
            &self.invocation_id,
            &self.caller,
        );
        if let Ok(mut events) = self.events.lock() {
            let _ = writeln!(events, "{event}");
        }
    }

    /// Only waits for an in-flight flush when the last successful one is stale.
    pub fn shutdown(&self) {
        let flush = self
            .stale_flush
            .lock()
            .ok()
            .and_then(|mut flush| flush.take());
        if let Some(flush) = flush {
            let _ = flush.recv_timeout(STALE_SHUTDOWN_WAIT);
        }
    }
}

fn flush(directory: &Path, key: HeaderValue, endpoint: &str) -> Option<()> {
    let touch = |name: &str| {
        File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join(name))
            .and_then(|file| file.set_modified(SystemTime::now()))
            .ok()
    };
    let lock = File::create(directory.join("flush.lock")).ok()?;
    lock.try_lock().ok()?;
    touch("attempted")?;

    let mut logs = Vec::new();
    for path in fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
    {
        let is_log = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("events-") && name.ends_with(".jsonl"));
        let Some(file) = is_log.then(|| File::open(&path).ok()).flatten() else {
            continue;
        };
        if file.try_lock().is_err() {
            continue;
        }
        let expired = file
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|time| time.elapsed().is_ok_and(|age| age > MAX_EVENT_AGE));
        if expired {
            drop(file);
            let _ = fs::remove_file(&path);
            continue;
        }
        logs.push((path, file));
    }

    let events: Vec<Value> = logs
        .iter()
        .flat_map(|(_, file)| BufReader::new(file).lines().map_while(Result::ok))
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    if !events.is_empty() {
        let mut headers = HeaderMap::new();
        headers.insert("statsig-api-key", key);
        headers.insert("statsig-sdk-type", HeaderValue::from_static("asimov-rust"));
        headers.insert(
            "statsig-sdk-version",
            HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        runtime.block_on(async {
            for batch in events.chunks(BATCH_SIZE) {
                client
                    .post(endpoint)
                    .header("statsig-client-time", timestamp().to_string())
                    .json(&json!({"events": batch}))
                    .send()
                    .await
                    .ok()?
                    .error_for_status()
                    .ok()?;
            }
            Some(())
        })?;
    }

    for (path, file) in logs {
        drop(file);
        let _ = fs::remove_file(path);
    }
    touch("flushed")
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct Caller {
    kind: &'static str,
    agent: &'static str,
    source: &'static str,
}

impl Caller {
    fn detect() -> Self {
        let Some(agent) = agent_detector::detect() else {
            return Self {
                kind: "unknown",
                agent: "unknown",
                source: "none",
            };
        };
        Self {
            kind: "agent",
            agent: Self::agent_name(&agent.name),
            source: match agent.source {
                agent_detector::DetectionSource::ParentProcess => "process",
                agent_detector::DetectionSource::StandardEnvVar
                | agent_detector::DetectionSource::ToolEnvVar => "environment",
                _ => "unknown",
            },
        }
    }

    fn agent_name(name: &str) -> &'static str {
        match name {
            "pi" => "pi",
            "hermes" => "hermes",
            "claude-code" => "claude-code",
            "codex" => "codex",
            "openclaw" => "openclaw",
            "opencode" => "opencode",
            "cursor" => "cursor",
            "cursor-cli" => "cursor-cli",
            "gemini" => "gemini",
            "cowork" => "cowork",
            "aider" => "aider",
            "github-copilot" => "github-copilot",
            "goose" => "goose",
            "cline" => "cline",
            "oh-my-pi" => "oh-my-pi",
            _ => "other",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct User {
    #[serde(rename = "userID")]
    user_id: String,
    app_version: String,
    custom: BTreeMap<&'static str, &'static str>,
}

fn wire_event(
    event: Event,
    time: u128,
    user: &User,
    invocation_id: &str,
    caller: &Caller,
) -> Value {
    let mut metadata = BTreeMap::from([
        ("schema_version", "1".to_string()),
        ("invocation_id", invocation_id.to_string()),
        ("caller_kind", caller.kind.to_string()),
        ("agent_name", caller.agent.to_string()),
        ("detection_source", caller.source.to_string()),
    ]);
    let name = match event {
        Event::Installed => "cli_installed",
        Event::CommandStarted { command, modules } => {
            metadata.insert("command", command);
            if let [module] = modules.as_slice() {
                metadata.insert("module", module.clone());
            }
            metadata.insert("modules", json!(modules).to_string());
            "cli_command_started"
        },
        Event::CommandFinished {
            command,
            modules,
            exit_code,
            duration,
        } => {
            metadata.insert("command", command);
            if let [module] = modules.as_slice() {
                metadata.insert("module", module.clone());
            }
            metadata.insert("modules", json!(modules).to_string());
            metadata.insert("exit_code", exit_code.to_string());
            metadata.insert(
                "outcome",
                if exit_code == 0 { "success" } else { "failure" }.to_string(),
            );
            metadata.insert("duration_ms", duration.as_millis().to_string());
            "cli_command_finished"
        },
        Event::ModuleOperationFinished {
            operation,
            module,
            outcome,
            duration,
        } => {
            metadata.insert(
                "operation",
                match operation {
                    crate::Operation::Fetch => "fetch",
                    crate::Operation::Read => "read",
                    crate::Operation::List => "list",
                }
                .to_string(),
            );
            metadata.insert("module", module);
            metadata.insert(
                "outcome",
                match outcome {
                    Outcome::Success => "success",
                    Outcome::Failure => "failure",
                }
                .to_string(),
            );
            metadata.insert("duration_ms", duration.as_millis().to_string());
            "cli_module_operation_finished"
        },
    };
    json!({"eventName": name, "user": user, "time": time, "metadata": metadata})
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn sends_logs_of_exited_invocations() {
        let directory = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let start = || Telemetry::start("client-test", "user".into(), "1", &directory, &endpoint);

        let previous = start().unwrap();
        previous.log(Event::Installed);
        previous.shutdown();
        drop(previous);
        fs::remove_file(directory.join("attempted")).unwrap();

        let current = start().unwrap();
        current.log(Event::Installed);
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !String::from_utf8_lossy(&request).contains("cli_installed") {
            let mut buffer = [0; 4096];
            let read = stream.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..read]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            let logs = || {
                fs::read_dir(&directory)
                    .unwrap()
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("events-"))
                    .count()
            };
            while logs() > 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let request = String::from_utf8_lossy(&request);
        assert_eq!(request.matches("cli_installed").count(), 1);
        assert!(directory.join("flushed").exists());
        drop(current);
        fs::remove_dir_all(directory).unwrap();
    }
}
