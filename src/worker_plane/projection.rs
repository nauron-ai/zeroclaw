use crate::config::Config;
#[cfg(feature = "worker-plane-distributed")]
use crate::spawned_runtime::{bootstrap_result_path, update_external_runtime_state};
use crate::worker_plane::worker_plane_enabled;
#[cfg(feature = "worker-plane-distributed")]
use crate::worker_plane::{download_bytes_from_artifact_ref, local_file_ref, MESSAGE_TYPE_HEADER};
#[cfg(feature = "worker-plane-distributed")]
use anyhow::Context;
use anyhow::Result;
#[cfg(feature = "worker-plane-distributed")]
use serde_json::Value;
#[cfg(feature = "worker-plane-distributed")]
use std::path::{Path, PathBuf};
#[cfg(feature = "worker-plane-distributed")]
use tokio::fs;
#[cfg(feature = "worker-plane-distributed")]
use tracing::warn;

#[cfg(feature = "worker-plane-distributed")]
use rdkafka::config::ClientConfig;
#[cfg(feature = "worker-plane-distributed")]
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
#[cfg(feature = "worker-plane-distributed")]
use rdkafka::message::{BorrowedMessage, Headers, Message};

pub async fn run_projection_loop(config: Config) -> Result<()> {
    #[cfg(not(feature = "worker-plane-distributed"))]
    {
        let _ = config;
        Ok(())
    }

    #[cfg(feature = "worker-plane-distributed")]
    {
        if !worker_plane_enabled(&config.worker_plane) {
            return Ok(());
        }

        let consumer = build_consumer(&config)?;
        consumer
            .subscribe(&[
                &config.worker_plane.redpanda.event_topic,
                &config.worker_plane.redpanda.heartbeat_topic,
            ])
            .context("Failed to subscribe worker-plane projector")?;

        loop {
            let message = consumer
                .recv()
                .await
                .context("Worker-plane projector consume failed")?;
            if let Err(error) = apply_projection_message(&config, &message).await {
                warn!(error = %error, "worker-plane projector: failed to process message");
            }
            consumer
                .commit_message(&message, CommitMode::Async)
                .context("Failed to commit worker-plane projection message")?;
        }
    }
}

#[cfg(feature = "worker-plane-distributed")]
fn build_consumer(config: &Config) -> Result<StreamConsumer> {
    let brokers = config.worker_plane.redpanda.brokers.join(",");
    if brokers.trim().is_empty() {
        anyhow::bail!("worker_plane.redpanda.brokers is empty");
    }

    ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set(
            "group.id",
            &config.worker_plane.redpanda.projection_consumer_group,
        )
        .set("auto.offset.reset", "earliest")
        .create()
        .context("Failed to create worker-plane projection consumer")
}

#[cfg(feature = "worker-plane-distributed")]
async fn apply_projection_message(config: &Config, message: &BorrowedMessage<'_>) -> Result<()> {
    let Some(message_type) = header_value(message, MESSAGE_TYPE_HEADER) else {
        return Ok(());
    };
    let payload = message
        .payload_view::<str>()
        .transpose()
        .context("Projection payload is not valid UTF-8")?
        .ok_or_else(|| anyhow::anyhow!("Projection payload is empty"))?;
    let json: Value =
        serde_json::from_str(payload).context("Failed to deserialize projection payload")?;
    let Some(agent_id) = json_string(&json, &["agent_id"]).or_else(|| message_key(message)) else {
        return Ok(());
    };

    let agent_home = spawned_agent_home(config, &agent_id);
    if !agent_home.exists() {
        return Ok(());
    }

    match message_type.as_str() {
        "AgentSpawned" => {
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "running".into();
                if state.task_status == "failed" {
                    state.task_status = "pending".into();
                }
                state.error = None;
            })
            .await?;
        }
        "AgentHeartbeat" => {
            let service_state = json_string(&json, &["service_state"]).unwrap_or_default();
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = match service_state.as_str() {
                    "starting" => "provisioning".into(),
                    "suspended" => "suspended".into(),
                    "terminated" => "terminated".into(),
                    "failed" => "failed".into(),
                    _ => "running".into(),
                };
            })
            .await?;
        }
        "AgentProgressReported" => {
            let stage = json_string(&json, &["stage"]).unwrap_or_default();
            let request_id = json_string(&json, &["request_id"]);
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "running".into();
                if matches!(stage.as_str(), "bootstrap_running" | "running") {
                    state.task_status = "running".into();
                }
                if let Some(request_id) = request_id {
                    state.current_request_id = Some(request_id.clone());
                    state.last_request_id = Some(request_id);
                }
                state.error = None;
            })
            .await?;
        }
        "AgentQuestionRaised" => {
            let request_id = json_string(&json, &["request_id"]);
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "running".into();
                state.task_status = "running".into();
                if let Some(request_id) = request_id {
                    state.current_request_id = Some(request_id.clone());
                    state.last_request_id = Some(request_id);
                }
            })
            .await?;
        }
        "AgentCompleted" => {
            let request_id = json_string(&json, &["request_id"]);
            let result_ref = json_string(&json, &["result_ref"]);
            let local_result_path =
                materialize_result_artifact(config, &agent_home, result_ref.as_deref()).await?;
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "running".into();
                state.task_status = "completed".into();
                state.current_request_id = None;
                if let Some(request_id) = request_id {
                    state.last_request_id = Some(request_id);
                }
                state.completed_at = Some(chrono::Utc::now().to_rfc3339());
                state.result_path = local_result_path.clone();
                state.error = None;
            })
            .await?;
        }
        "AgentSpawnFailed" => {
            let request_id = json_string(&json, &["request_id"]);
            let error = json_string(&json, &["error"]);
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "failed".into();
                state.task_status = "failed".into();
                state.current_request_id = None;
                if let Some(request_id) = request_id {
                    state.last_request_id = Some(request_id);
                }
                state.completed_at = Some(chrono::Utc::now().to_rfc3339());
                state.error = error;
            })
            .await?;
        }
        "AgentSuspended" => {
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "suspended".into();
            })
            .await?;
        }
        "AgentTerminated" => {
            update_external_runtime_state(&agent_home, &agent_id, |state| {
                state.lifecycle_status = "terminated".into();
                state.current_request_id = None;
                state.completed_at = Some(chrono::Utc::now().to_rfc3339());
            })
            .await?;
        }
        _ => {}
    }

    Ok(())
}

#[cfg(feature = "worker-plane-distributed")]
async fn materialize_result_artifact(
    config: &Config,
    agent_home: &Path,
    result_ref: Option<&str>,
) -> Result<Option<String>> {
    let Some(result_ref) = result_ref else {
        return Ok(None);
    };

    match download_bytes_from_artifact_ref(&config.worker_plane, result_ref).await {
        Ok(bytes) => {
            let path = bootstrap_result_path(agent_home);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::write(&path, bytes)
                .await
                .with_context(|| format!("Failed to write {}", path.display()))?;
            Ok(Some(local_file_ref(&path)))
        }
        Err(error) => {
            warn!(result_ref, error = %error, "worker-plane projector: failed to materialize result artifact");
            Ok(Some(result_ref.to_string()))
        }
    }
}

#[cfg(feature = "worker-plane-distributed")]
fn spawned_agent_home(config: &Config, agent_id: &str) -> PathBuf {
    config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("spawned-agents")
        .join(agent_id)
}

#[cfg(feature = "worker-plane-distributed")]
fn header_value(message: &BorrowedMessage<'_>, key: &str) -> Option<String> {
    let headers = message.headers()?;
    for index in 0..headers.count() {
        let header = headers.try_get(index)?;
        if header.key != key {
            continue;
        }
        if let Some(value) = header.value {
            return Some(String::from_utf8_lossy(value).into_owned());
        }
    }
    None
}

#[cfg(feature = "worker-plane-distributed")]
fn message_key(message: &BorrowedMessage<'_>) -> Option<String> {
    message
        .key_view::<str>()
        .transpose()
        .ok()
        .flatten()
        .map(ToOwned::to_owned)
}

#[cfg(feature = "worker-plane-distributed")]
fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().map(ToOwned::to_owned)
}
