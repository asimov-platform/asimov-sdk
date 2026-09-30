// This is free and unencumbered software released into the public domain.

use crate::{Event, Outcome};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
};
use core::time::Duration;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const ENDPOINT: &str = "https://api.statsig.com/v1/log_event";

#[derive(Clone)]
pub struct Telemetry {
    sender: mpsc::Sender<(Event, u128)>,
}

impl Telemetry {
    /// Starts a detached delivery worker. Dropping the client never waits for delivery.
    pub fn new(client_key: &str, user_id: String, app_version: &str) -> Option<Self> {
        Self::start(client_key, user_id, app_version, ENDPOINT)
    }

    fn start(client_key: &str, user_id: String, app_version: &str, endpoint: &str) -> Option<Self> {
        if !client_key.starts_with("client-") || client_key.len() <= "client-".len() {
            return None;
        }
        let mut key = HeaderValue::from_str(client_key).ok()?;
        key.set_sensitive(true);
        let (sender, mut receiver) = mpsc::channel(64);
        let app_version = app_version.to_string();
        let endpoint = endpoint.to_string();
        let invocation_id = uuid::Uuid::new_v4().to_string();
        std::thread::Builder::new()
            .name("asimov-telemetry".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    let mut headers = HeaderMap::new();
                    headers.insert("statsig-api-key", key);
                    headers.insert("statsig-sdk-type", HeaderValue::from_static("asimov-rust"));
                    headers.insert(
                        "statsig-sdk-version",
                        HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
                    );
                    let Ok(client) = reqwest::Client::builder()
                        .default_headers(headers)
                        .redirect(reqwest::redirect::Policy::none())
                        .timeout(Duration::from_secs(3))
                        .build()
                    else {
                        return;
                    };
                    let caller = Caller::detect();
                    let user = User {
                        user_id,
                        app_version,
                        custom: BTreeMap::from([
                            ("target_os", std::env::consts::OS),
                            ("target_arch", std::env::consts::ARCH),
                        ]),
                    };
                    while let Some((event, time)) = receiver.recv().await {
                        let event = wire_event(event, time, &user, &invocation_id, &caller);
                        let _ = client
                            .post(&endpoint)
                            .header("statsig-client-time", timestamp().to_string())
                            .json(&json!({"events": [event]}))
                            .send()
                            .await;
                    }
                });
            })
            .ok()?;
        Some(Self { sender })
    }

    /// Drops the event if delivery is unavailable or the in-memory queue is full.
    pub fn log(&self, event: Event) {
        let _ = self.sender.try_send((event, timestamp()));
    }
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
