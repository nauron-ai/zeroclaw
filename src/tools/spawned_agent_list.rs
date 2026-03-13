use super::spawned_agent_registry::{
    discover_all_status_snapshots, SpawnedAgentRegistry, SpawnedAgentServiceState,
    SpawnedAgentSummary,
};
use super::traits::{Tool, ToolResult};
use crate::runtime::DockerAgentSpawner;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct SpawnedAgentListTool {
    registry: Arc<SpawnedAgentRegistry>,
    spawner: DockerAgentSpawner,
    labaclaw_dir: PathBuf,
}

impl SpawnedAgentListTool {
    pub fn new(
        registry: Arc<SpawnedAgentRegistry>,
        spawner: DockerAgentSpawner,
        labaclaw_dir: PathBuf,
    ) -> Self {
        Self {
            registry,
            spawner,
            labaclaw_dir,
        }
    }
}

#[async_trait]
impl Tool for SpawnedAgentListTool {
    fn name(&self) -> &str {
        "spawned_agent_list"
    }

    fn description(&self) -> &str {
        "List dedicated child agents spawned by the orchestrator together with their service and task state."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let mut merged = BTreeMap::new();
        for summary in self.registry.list() {
            merged.insert(summary.agent_id.clone(), summary);
        }

        for mut snapshot in discover_all_status_snapshots(&self.labaclaw_dir)? {
            if let Some(state) = self.spawner.inspect_service(&snapshot.container_name).await? {
                snapshot.service_state = match state.state.as_str() {
                    "running" => SpawnedAgentServiceState::Running,
                    "exited" | "dead" => SpawnedAgentServiceState::Terminated,
                    "paused" => SpawnedAgentServiceState::Suspended,
                    _ => snapshot.service_state,
                };
                snapshot.container_id = Some(state.container_id);
            }
            merged.entry(snapshot.agent_id.clone()).or_insert_with(|| SpawnedAgentSummary {
                agent_id: snapshot.agent_id,
                display_name: snapshot.display_name,
                pack_id: snapshot.pack_id,
                task_profile: snapshot.task_profile,
                lifecycle_mode: snapshot.lifecycle_mode,
                primary_provider: snapshot.primary_provider,
                primary_model: snapshot.primary_model,
                service_state: snapshot.service_state.as_str().to_string(),
                task_state: snapshot.task_state.as_str().to_string(),
                started_at: snapshot.started_at.to_rfc3339(),
                updated_at: snapshot.updated_at.to_rfc3339(),
            });
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(
                &merged.into_values().collect::<Vec<SpawnedAgentSummary>>(),
            )?,
            error: None,
        })
    }
}
