use crate::{agent::Agent, Config};
use crate::worker_plane::{local_file_ref, summarize_text_for_event};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

const RUNTIME_DIR: &str = "runtime";
const BOOTSTRAP_REQUEST_FILE: &str = "bootstrap-request.json";
const BOOTSTRAP_ACTIVE_FILE: &str = "bootstrap-request.active.json";
const BOOTSTRAP_DONE_FILE: &str = "bootstrap-request.done.json";
const RUNTIME_STATE_FILE: &str = "runtime_state.json";
const BOOTSTRAP_RESULT_FILE: &str = "BOOTSTRAP_RESULT.md";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnedAgentTaskRequest {
    pub request_id: String,
    pub message: String,
    pub max_history_messages: Option<usize>,
    pub max_tool_iterations: Option<usize>,
    pub compact_context: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnedAgentRuntimeState {
    pub agent_id: String,
    pub lifecycle_status: String,
    pub task_status: String,
    pub current_request_id: Option<String>,
    pub last_request_id: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub last_heartbeat_at: String,
    pub completed_at: Option<String>,
    pub result_path: Option<String>,
    pub error: Option<String>,
}

impl SpawnedAgentRuntimeState {
    fn new(agent_id: String) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            agent_id,
            lifecycle_status: "idle".into(),
            task_status: "pending".into(),
            current_request_id: None,
            last_request_id: None,
            started_at: now.clone(),
            updated_at: now.clone(),
            last_heartbeat_at: now,
            completed_at: None,
            result_path: None,
            error: None,
        }
    }
}

pub fn agent_home_from_config(config: &Config) -> PathBuf {
    config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

pub fn runtime_dir(agent_home: &Path) -> PathBuf {
    agent_home.join(RUNTIME_DIR)
}

pub fn bootstrap_request_path(agent_home: &Path) -> PathBuf {
    runtime_dir(agent_home).join(BOOTSTRAP_REQUEST_FILE)
}

pub fn bootstrap_active_request_path(agent_home: &Path) -> PathBuf {
    runtime_dir(agent_home).join(BOOTSTRAP_ACTIVE_FILE)
}

pub fn bootstrap_done_request_path(agent_home: &Path) -> PathBuf {
    runtime_dir(agent_home).join(BOOTSTRAP_DONE_FILE)
}

pub fn runtime_state_path(agent_home: &Path) -> PathBuf {
    agent_home.join(RUNTIME_STATE_FILE)
}

pub fn bootstrap_result_path(agent_home: &Path) -> PathBuf {
    agent_home.join("workspace").join(BOOTSTRAP_RESULT_FILE)
}

pub async fn write_task_request(
    agent_home: &Path,
    request: &SpawnedAgentTaskRequest,
) -> Result<PathBuf> {
    let dir = runtime_dir(agent_home);
    fs::create_dir_all(&dir).await?;
    let path = bootstrap_request_path(agent_home);
    fs::write(
        &path,
        serde_json::to_vec_pretty(request).context("Failed to serialize task request")?,
    )
    .await?;
    Ok(path)
}

pub async fn read_runtime_state(path: &Path) -> Result<Option<SpawnedAgentRuntimeState>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).await?;
    let state = serde_json::from_slice(&bytes).context("Failed to parse runtime state")?;
    Ok(Some(state))
}

pub async fn run(config: Config, poll_interval_ms: u64) -> Result<()> {
    let agent_home = agent_home_from_config(&config);
    fs::create_dir_all(runtime_dir(&agent_home)).await?;
    fs::create_dir_all(agent_home.join("events")).await?;

    let state_path = runtime_state_path(&agent_home);
    let mut state = read_runtime_state(&state_path)
        .await?
        .unwrap_or_else(|| SpawnedAgentRuntimeState::new(agent_id_from_home(&agent_home)));
    refresh_heartbeat(&mut state);
    if state.lifecycle_status == "running" {
        state.lifecycle_status = "idle".into();
    }
    persist_state(&state_path, &state).await?;

    let poll = Duration::from_millis(poll_interval_ms.max(250));

    loop {
        if let Some((request, active_path)) = take_request(&agent_home).await? {
            process_request(
                &config,
                &agent_home,
                &state_path,
                &mut state,
                &request,
                &active_path,
            )
            .await?;
        } else {
            if state.lifecycle_status != "running" {
                state.lifecycle_status = "idle".into();
            }
            refresh_heartbeat(&mut state);
            persist_state(&state_path, &state).await?;
            sleep(poll).await;
        }
    }
}

async fn take_request(agent_home: &Path) -> Result<Option<(SpawnedAgentTaskRequest, PathBuf)>> {
    let active = bootstrap_active_request_path(agent_home);
    if active.exists() {
        let request = read_task_request(&active).await?;
        return Ok(Some((request, active)));
    }

    let pending = bootstrap_request_path(agent_home);
    if !pending.exists() {
        return Ok(None);
    }

    if active.exists() {
        let _ = fs::remove_file(&active).await;
    }
    fs::rename(&pending, &active)
        .await
        .with_context(|| format!("Failed to move {} into active state", pending.display()))?;
    let request = read_task_request(&active).await?;
    Ok(Some((request, active)))
}

async fn read_task_request(path: &Path) -> Result<SpawnedAgentTaskRequest> {
    let bytes = fs::read(path).await?;
    serde_json::from_slice(&bytes).context("Failed to parse task request")
}

async fn process_request(
    config: &Config,
    agent_home: &Path,
    state_path: &Path,
    state: &mut SpawnedAgentRuntimeState,
    request: &SpawnedAgentTaskRequest,
    active_path: &Path,
) -> Result<()> {
    state.lifecycle_status = "running".into();
    state.task_status = "running".into();
    state.current_request_id = Some(request.request_id.clone());
    state.last_request_id = Some(request.request_id.clone());
    state.completed_at = None;
    state.error = None;
    state.result_path = None;
    refresh_heartbeat(state);
    persist_state(state_path, state).await?;
    write_event(
        agent_home,
        "AgentProgressReported",
        json!({
            "event_id": Uuid::new_v4().to_string(),
            "agent_id": state.agent_id,
            "stage": "running",
            "detail": "Spawned agent runtime started initial mission",
            "request_id": request.request_id,
            "reported_at": Utc::now().to_rfc3339(),
        }),
    )
    .await?;

    let mut effective = config.clone();
    if let Some(limit) = request.max_history_messages {
        effective.agent.max_history_messages = limit;
    }
    if let Some(limit) = request.max_tool_iterations {
        effective.agent.max_tool_iterations = limit;
    }
    if request.compact_context {
        effective.agent.compact_context = true;
    }

    let result_path = bootstrap_result_path(agent_home);
    let outcome = async {
        let mut agent = Agent::from_config(&effective)?;
        agent.run_single(&request.message).await
    }
    .await;

    if let Some(parent) = result_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    match outcome {
        Ok(output) => {
            fs::write(&result_path, &output).await?;
            let result_ref = local_file_ref(&result_path);
            state.lifecycle_status = "idle".into();
            state.task_status = "completed".into();
            state.current_request_id = None;
            state.completed_at = Some(Utc::now().to_rfc3339());
            state.result_path = Some(result_ref.clone());
            refresh_heartbeat(state);
            persist_state(state_path, state).await?;
            finalize_request(agent_home, active_path).await?;
            write_event(
                agent_home,
                "AgentCompleted",
                json!({
                    "event_id": Uuid::new_v4().to_string(),
                    "agent_id": state.agent_id,
                    "request_id": request.request_id,
                    "result_ref": result_ref,
                    "summary": summarize_text_for_event(&output),
                }),
            )
            .await?;
        }
        Err(error) => {
            state.lifecycle_status = "idle".into();
            state.task_status = "failed".into();
            state.current_request_id = None;
            state.completed_at = Some(Utc::now().to_rfc3339());
            state.error = Some(error.to_string());
            refresh_heartbeat(state);
            persist_state(state_path, state).await?;
            finalize_request(agent_home, active_path).await?;
            write_event(
                agent_home,
                "AgentSpawnFailed",
                json!({
                    "event_id": Uuid::new_v4().to_string(),
                    "agent_id": state.agent_id,
                    "request_id": request.request_id,
                    "error": error.to_string(),
                    "failed_at": Utc::now().to_rfc3339(),
                }),
            )
            .await?;
        }
    }

    Ok(())
}

async fn finalize_request(agent_home: &Path, active_path: &Path) -> Result<()> {
    let done_path = bootstrap_done_request_path(agent_home);
    if done_path.exists() {
        let _ = fs::remove_file(&done_path).await;
    }
    fs::rename(active_path, done_path)
        .await
        .context("Failed to finalize processed task request")?;
    Ok(())
}

async fn persist_state(path: &Path, state: &SpawnedAgentRuntimeState) -> Result<()> {
    fs::write(
        path,
        serde_json::to_vec_pretty(state).context("Failed to serialize runtime state")?,
    )
    .await?;
    Ok(())
}

fn refresh_heartbeat(state: &mut SpawnedAgentRuntimeState) {
    let now = Utc::now().to_rfc3339();
    state.updated_at = now.clone();
    state.last_heartbeat_at = now;
}

fn agent_id_from_home(agent_home: &Path) -> String {
    agent_home
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "spawned-agent".into())
}

async fn write_event(
    agent_home: &Path,
    event_name: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let events_dir = agent_home.join("events");
    fs::create_dir_all(&events_dir).await?;
    let file_name = format!(
        "{}-{}.v1.json",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        event_name
    );
    fs::write(
        events_dir.join(file_name),
        serde_json::to_vec_pretty(&payload).context("Failed to serialize runtime event")?,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_paths_live_under_agent_home() {
        let home = PathBuf::from("/tmp/demo-agent");
        assert_eq!(
            bootstrap_request_path(&home),
            home.join("runtime/bootstrap-request.json")
        );
        assert_eq!(runtime_state_path(&home), home.join("runtime_state.json"));
        assert_eq!(
            bootstrap_result_path(&home),
            home.join("workspace/BOOTSTRAP_RESULT.md")
        );
    }
}
