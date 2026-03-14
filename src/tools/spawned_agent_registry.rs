use crate::tools::traits::ToolResult;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnedAgentServiceState {
    Provisioning,
    Running,
    Suspended,
    Failed,
    Terminated,
}

impl SpawnedAgentServiceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Provisioning => "provisioning",
            Self::Running => "running",
            Self::Suspended => "suspended",
            Self::Failed => "failed",
            Self::Terminated => "terminated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnedAgentTaskState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SpawnedAgentTaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SpawnedAgentProgressEntry {
    pub at: String,
    pub stage: String,
    pub detail: String,
}

pub struct SpawnedAgentSession {
    pub agent_id: String,
    pub display_name: String,
    pub owner_agent_id: String,
    pub pack_id: String,
    pub task_profile: String,
    pub lifecycle_mode: String,
    pub primary_provider: String,
    pub primary_model: String,
    pub local_route_hints: Vec<String>,
    pub task: String,
    pub config_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub runtime_state_path: PathBuf,
    pub container_name: String,
    pub container_id: Option<String>,
    pub service_state: SpawnedAgentServiceState,
    pub task_state: SpawnedAgentTaskState,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<ToolResult>,
    pub last_error: Option<String>,
    pub progress: Vec<SpawnedAgentProgressEntry>,
    pub handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct SpawnedAgentRegistry {
    sessions: Arc<RwLock<HashMap<String, SpawnedAgentSession>>>,
}

impl SpawnedAgentRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn insert(&self, session: SpawnedAgentSession) {
        self.sessions
            .write()
            .insert(session.agent_id.clone(), session);
    }

    pub fn set_handle(&self, agent_id: &str, handle: JoinHandle<()>) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.handle = Some(handle);
            session.updated_at = Utc::now();
        }
    }

    pub fn clear_handle(&self, agent_id: &str) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.handle = None;
            session.updated_at = Utc::now();
        }
    }

    pub fn append_progress(&self, agent_id: &str, stage: &str, detail: impl Into<String>) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.progress.push(SpawnedAgentProgressEntry {
                at: Utc::now().to_rfc3339(),
                stage: stage.to_string(),
                detail: detail.into(),
            });
            session.updated_at = Utc::now();
        }
    }

    pub fn mark_service_running(&self, agent_id: &str, container_id: String) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.container_id = Some(container_id);
            session.service_state = SpawnedAgentServiceState::Running;
            session.updated_at = Utc::now();
        }
    }

    pub fn mark_service_state(&self, agent_id: &str, service_state: SpawnedAgentServiceState) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.service_state = service_state;
            session.updated_at = Utc::now();
            if matches!(session.service_state, SpawnedAgentServiceState::Terminated) {
                session.completed_at = Some(Utc::now());
            }
        }
    }

    pub fn mark_task_running(&self, agent_id: &str) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.task_state = SpawnedAgentTaskState::Running;
            session.updated_at = Utc::now();
        }
    }

    pub fn complete_task(&self, agent_id: &str, result: ToolResult) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.task_state = SpawnedAgentTaskState::Completed;
            session.result = Some(result);
            session.completed_at = Some(Utc::now());
            session.updated_at = Utc::now();
            session.handle = None;
        }
    }

    pub fn fail_task(&self, agent_id: &str, error: String) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            session.task_state = SpawnedAgentTaskState::Failed;
            session.last_error = Some(error.clone());
            session.result = Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
            session.completed_at = Some(Utc::now());
            session.updated_at = Utc::now();
            session.handle = None;
        }
    }

    pub fn cancel_task(&self, agent_id: &str, error: String) {
        if let Some(session) = self.sessions.write().get_mut(agent_id) {
            if let Some(handle) = session.handle.take() {
                handle.abort();
            }
            session.task_state = SpawnedAgentTaskState::Cancelled;
            session.last_error = Some(error.clone());
            session.result = Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
            session.completed_at = Some(Utc::now());
            session.updated_at = Utc::now();
        }
    }

    pub fn exists(&self, agent_id: &str) -> bool {
        self.sessions.read().contains_key(agent_id)
    }

    pub fn get_status(&self, agent_id: &str) -> Option<SpawnedAgentStatusSnapshot> {
        self.sessions
            .read()
            .get(agent_id)
            .map(SpawnedAgentStatusSnapshot::from_session)
    }

    pub fn list(&self) -> Vec<SpawnedAgentSummary> {
        self.sessions
            .read()
            .values()
            .map(SpawnedAgentSummary::from_session)
            .collect()
    }
}

impl Default for SpawnedAgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SpawnedAgentSummary {
    pub agent_id: String,
    pub display_name: String,
    pub pack_id: String,
    pub task_profile: String,
    pub lifecycle_mode: String,
    pub primary_provider: String,
    pub primary_model: String,
    pub container_id: Option<String>,
    pub service_state: String,
    pub task_state: String,
    pub started_at: String,
    pub updated_at: String,
}

impl SpawnedAgentSummary {
    fn from_session(session: &SpawnedAgentSession) -> Self {
        Self {
            agent_id: session.agent_id.clone(),
            display_name: session.display_name.clone(),
            pack_id: session.pack_id.clone(),
            task_profile: session.task_profile.clone(),
            lifecycle_mode: session.lifecycle_mode.clone(),
            primary_provider: session.primary_provider.clone(),
            primary_model: session.primary_model.clone(),
            container_id: session.container_id.clone(),
            service_state: session.service_state.as_str().to_string(),
            task_state: session.task_state.as_str().to_string(),
            started_at: session.started_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpawnedAgentStatusSnapshot {
    pub agent_id: String,
    pub display_name: String,
    pub owner_agent_id: String,
    pub pack_id: String,
    pub task_profile: String,
    pub lifecycle_mode: String,
    pub primary_provider: String,
    pub primary_model: String,
    pub local_route_hints: Vec<String>,
    pub task: String,
    pub config_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub runtime_state_path: PathBuf,
    pub container_name: String,
    pub container_id: Option<String>,
    pub service_state: SpawnedAgentServiceState,
    pub task_state: SpawnedAgentTaskState,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<ToolResult>,
    pub last_error: Option<String>,
    pub progress: Vec<SpawnedAgentProgressEntry>,
}

impl SpawnedAgentStatusSnapshot {
    fn from_session(session: &SpawnedAgentSession) -> Self {
        Self {
            agent_id: session.agent_id.clone(),
            display_name: session.display_name.clone(),
            owner_agent_id: session.owner_agent_id.clone(),
            pack_id: session.pack_id.clone(),
            task_profile: session.task_profile.clone(),
            lifecycle_mode: session.lifecycle_mode.clone(),
            primary_provider: session.primary_provider.clone(),
            primary_model: session.primary_model.clone(),
            local_route_hints: session.local_route_hints.clone(),
            task: session.task.clone(),
            config_dir: session.config_dir.clone(),
            workspace_dir: session.workspace_dir.clone(),
            runtime_state_path: session.runtime_state_path.clone(),
            container_name: session.container_name.clone(),
            container_id: session.container_id.clone(),
            service_state: session.service_state.clone(),
            task_state: session.task_state.clone(),
            started_at: session.started_at,
            updated_at: session.updated_at,
            completed_at: session.completed_at,
            result: session.result.clone(),
            last_error: session.last_error.clone(),
            progress: session.progress.clone(),
        }
    }
}

pub fn discover_status_snapshot(
    labaclaw_dir: &Path,
    agent_id: &str,
) -> Result<Option<SpawnedAgentStatusSnapshot>> {
    let agent_id_path = validated_agent_id_path(agent_id)?;
    let agent_home = spawned_agents_dir(labaclaw_dir).join(agent_id_path);
    if !agent_home.exists() {
        return Ok(None);
    }

    Ok(Some(load_snapshot_from_agent_home(&agent_home)?))
}

pub fn discover_all_status_snapshots(
    labaclaw_dir: &Path,
) -> Result<Vec<SpawnedAgentStatusSnapshot>> {
    let root = spawned_agents_dir(labaclaw_dir);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();
    for entry in fs::read_dir(&root)
        .with_context(|| format!("Failed to read spawned agents dir {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        snapshots.push(load_snapshot_from_agent_home(&path)?);
    }
    snapshots.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    Ok(snapshots)
}

fn spawned_agents_dir(labaclaw_dir: &Path) -> PathBuf {
    labaclaw_dir.join("spawned-agents")
}

fn validated_agent_id_path(agent_id: &str) -> Result<&Path> {
    let agent_id_path = Path::new(agent_id);
    if agent_id_path.is_absolute()
        || agent_id_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        anyhow::bail!("Invalid spawned agent id");
    }
    Ok(agent_id_path)
}

fn load_snapshot_from_agent_home(agent_home: &Path) -> Result<SpawnedAgentStatusSnapshot> {
    let spec_path = latest_agent_spec_path(agent_home)
        .with_context(|| format!("No agent-spec.json found under {}", agent_home.display()))?;
    let spec = read_json(&spec_path)?;
    let runtime_state_path = agent_home.join("runtime_state.json");
    let runtime_state = if runtime_state_path.exists() {
        Some(read_json(&runtime_state_path)?)
    } else {
        None
    };
    let result_path = agent_home.join("workspace").join("BOOTSTRAP_RESULT.md");
    let result_output = if result_path.exists() {
        Some(
            fs::read_to_string(&result_path)
                .with_context(|| format!("Failed to read {}", result_path.display()))?,
        )
    } else {
        None
    };

    let agent_id = json_string(&spec, &["agent_id"]).unwrap_or_else(|| {
        agent_home
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into())
    });
    let lifecycle_mode =
        json_string(&spec, &["lifecycle_mode"]).unwrap_or_else(|| "dedicated".into());
    let task_state = parse_task_state(
        runtime_state
            .as_ref()
            .and_then(|value| json_string(value, &["task_status"])),
    );
    let service_state = parse_service_state(
        runtime_state
            .as_ref()
            .and_then(|value| json_string(value, &["lifecycle_status"])),
        &lifecycle_mode,
        &task_state,
    );
    let started_at = parse_datetime(
        runtime_state
            .as_ref()
            .and_then(|value| json_string(value, &["started_at"]))
            .or_else(|| json_string(&spec, &["created_at"])),
    );
    let updated_at = parse_datetime(
        runtime_state
            .as_ref()
            .and_then(|value| json_string(value, &["updated_at"]))
            .or_else(|| {
                runtime_state
                    .as_ref()
                    .and_then(|value| json_string(value, &["last_heartbeat_at"]))
            })
            .or_else(|| json_string(&spec, &["created_at"])),
    );
    let completed_at = runtime_state
        .as_ref()
        .and_then(|value| json_string(value, &["completed_at"]))
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc));
    let local_route_hints = spec
        .get("local_model_routes")
        .and_then(Value::as_array)
        .map(|routes| {
            routes
                .iter()
                .filter_map(|route| json_string(route, &["hint"]))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let last_error = runtime_state
        .as_ref()
        .and_then(|value| json_string(value, &["error"]));

    Ok(SpawnedAgentStatusSnapshot {
        agent_id: agent_id.clone(),
        display_name: json_string(&spec, &["display_name"]).unwrap_or_else(|| agent_id.clone()),
        owner_agent_id: json_string(&spec, &["owner_agent_id"])
            .unwrap_or_else(|| "orchestrator".into()),
        pack_id: json_string(&spec, &["pack_id"]).unwrap_or_else(|| "general_specialist".into()),
        task_profile: json_string(&spec, &["task_profile"])
            .unwrap_or_else(|| "fast_conversational".into()),
        lifecycle_mode,
        primary_provider: json_string(&spec, &["primary_llm", "provider"])
            .unwrap_or_else(|| "unknown".into()),
        primary_model: json_string(&spec, &["primary_llm", "model"])
            .unwrap_or_else(|| "unknown".into()),
        local_route_hints,
        task: json_string(&spec, &["initial_mission"]).unwrap_or_default(),
        config_dir: agent_home.to_path_buf(),
        workspace_dir: agent_home.join("workspace"),
        runtime_state_path,
        container_name: format!("labaclaw-agent-{agent_id}"),
        container_id: None,
        service_state,
        task_state: task_state.clone(),
        started_at,
        updated_at,
        completed_at,
        result: result_output.map(|output| ToolResult {
            success: task_state == SpawnedAgentTaskState::Completed,
            output,
            error: last_error.clone(),
        }),
        last_error,
        progress: Vec::new(),
    })
}

fn latest_agent_spec_path(agent_home: &Path) -> Result<PathBuf> {
    let specs_dir = agent_home.join("specs");
    let mut spec_paths = fs::read_dir(&specs_dir)
        .with_context(|| format!("Failed to read specs dir {}", specs_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("agent-spec.json"))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    spec_paths.sort();
    spec_paths
        .pop()
        .context("No versioned agent-spec.json found")
}

fn read_json(path: &Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().map(ToOwned::to_owned)
}

fn parse_task_state(raw: Option<String>) -> SpawnedAgentTaskState {
    match raw.as_deref() {
        Some("running") => SpawnedAgentTaskState::Running,
        Some("completed") => SpawnedAgentTaskState::Completed,
        Some("failed") => SpawnedAgentTaskState::Failed,
        Some("cancelled") => SpawnedAgentTaskState::Cancelled,
        _ => SpawnedAgentTaskState::Pending,
    }
}

fn parse_service_state(
    raw: Option<String>,
    lifecycle_mode: &str,
    task_state: &SpawnedAgentTaskState,
) -> SpawnedAgentServiceState {
    match raw.as_deref() {
        Some("running") => SpawnedAgentServiceState::Running,
        Some("suspended") => SpawnedAgentServiceState::Suspended,
        Some("failed") => SpawnedAgentServiceState::Failed,
        Some("terminated") => SpawnedAgentServiceState::Terminated,
        _ if matches!(task_state, SpawnedAgentTaskState::Failed) => {
            SpawnedAgentServiceState::Failed
        }
        _ if matches!(task_state, SpawnedAgentTaskState::Cancelled) => {
            SpawnedAgentServiceState::Terminated
        }
        _ if matches!(task_state, SpawnedAgentTaskState::Completed)
            && lifecycle_mode == "dedicated" =>
        {
            SpawnedAgentServiceState::Running
        }
        _ if matches!(task_state, SpawnedAgentTaskState::Completed) => {
            SpawnedAgentServiceState::Terminated
        }
        _ => SpawnedAgentServiceState::Provisioning,
    }
}

fn parse_datetime(raw: Option<String>) -> DateTime<Utc> {
    raw.and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_status_snapshot_rejects_traversal_agent_ids() {
        let labaclaw_dir = tempfile::tempdir().unwrap();

        let error =
            discover_status_snapshot(labaclaw_dir.path(), "../outside").expect_err("must reject");
        assert!(error.to_string().contains("Invalid spawned agent id"));
    }

    #[test]
    fn parse_service_state_inferrs_terminal_states_from_task_state() {
        assert_eq!(
            parse_service_state(None, "dedicated", &SpawnedAgentTaskState::Failed),
            SpawnedAgentServiceState::Failed
        );
        assert_eq!(
            parse_service_state(None, "ephemeral", &SpawnedAgentTaskState::Cancelled),
            SpawnedAgentServiceState::Terminated
        );
        assert_eq!(
            parse_service_state(None, "ephemeral", &SpawnedAgentTaskState::Completed),
            SpawnedAgentServiceState::Terminated
        );
    }
}
