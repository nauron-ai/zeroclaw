use super::spawned_agent_registry::{
    discover_status_snapshot, SpawnedAgentRegistry, SpawnedAgentServiceState,
    SpawnedAgentStatusSnapshot, SpawnedAgentTaskState,
};
use super::traits::{Tool, ToolResult};
use crate::config::Config;
use crate::runtime::DockerAgentSpawner;
use crate::security::policy::ToolOperation;
use crate::security::SecurityPolicy;
use crate::worker_plane::{
    distributed_spawn_plan_path, publish_json_message, DistributedSpawnPlan,
    COMMAND_TYPE_RESUME_AGENT_REQUESTED, COMMAND_TYPE_SUSPEND_AGENT_REQUESTED,
    COMMAND_TYPE_TERMINATE_AGENT_REQUESTED,
};
use anyhow::Context;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct SpawnedAgentManageTool {
    registry: Arc<SpawnedAgentRegistry>,
    security: Arc<SecurityPolicy>,
    spawner: DockerAgentSpawner,
    labaclaw_dir: PathBuf,
    root_config: Arc<Config>,
}

impl SpawnedAgentManageTool {
    pub fn new(
        registry: Arc<SpawnedAgentRegistry>,
        security: Arc<SecurityPolicy>,
        spawner: DockerAgentSpawner,
        labaclaw_dir: PathBuf,
        root_config: Arc<Config>,
    ) -> Self {
        Self {
            registry,
            security,
            spawner,
            labaclaw_dir,
            root_config,
        }
    }
}

#[async_trait]
impl Tool for SpawnedAgentManageTool {
    fn name(&self) -> &str {
        "spawned_agent_manage"
    }

    fn description(&self) -> &str {
        "Manage a dedicated child agent. Actions: status, suspend, resume, terminate."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "agent_id": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Dedicated child agent identifier returned by spawn_agent"
                },
                "action": {
                    "type": "string",
                    "enum": ["status", "suspend", "resume", "terminate"],
                    "description": "Lifecycle action to execute"
                }
            },
            "required": ["agent_id", "action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let agent_id = args
            .get("agent_id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing or empty 'agent_id' parameter"))?;
        let action = args
            .get("action")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing or empty 'action' parameter"))?;

        match action {
            "status" => self.handle_status(agent_id).await,
            "suspend" => self.handle_suspend(agent_id).await,
            "resume" => self.handle_resume(agent_id).await,
            "terminate" => self.handle_terminate(agent_id).await,
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{other}'. Must be one of: status, suspend, resume, terminate"
                )),
            }),
        }
    }
}

impl SpawnedAgentManageTool {
    async fn handle_status(&self, agent_id: &str) -> anyhow::Result<ToolResult> {
        let Some(mut snapshot) = self.resolve_snapshot(agent_id)? else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown spawned agent '{agent_id}'")),
            });
        };

        let docker_state = self
            .spawner
            .inspect_service(&snapshot.container_name)
            .await?;
        if let Some(state) = &docker_state {
            snapshot.container_id = Some(state.container_id.clone());
            snapshot.service_state = match state.state.as_str() {
                "running" => SpawnedAgentServiceState::Running,
                "exited" | "dead" => SpawnedAgentServiceState::Terminated,
                "paused" => SpawnedAgentServiceState::Suspended,
                _ => snapshot.service_state.clone(),
            };
        }
        let runtime_state = if snapshot.runtime_state_path.exists() {
            Some(
                serde_json::from_slice::<serde_json::Value>(
                    &tokio::fs::read(&snapshot.runtime_state_path).await?,
                )
                .unwrap_or_else(|_| json!({"error": "Failed to parse runtime_state.json"})),
            )
        } else {
            None
        };
        let distributed_plan_path = distributed_spawn_plan_path(&snapshot.config_dir);
        let distributed_plan = if distributed_plan_path.exists() {
            Some(
                serde_json::from_slice::<serde_json::Value>(
                    &tokio::fs::read(&distributed_plan_path).await?,
                )
                .unwrap_or_else(|_| json!({"error": "Failed to parse spawn-plan.json"})),
            )
        } else {
            None
        };

        let mut output = json!({
            "agent_id": snapshot.agent_id,
            "display_name": snapshot.display_name,
            "owner_agent_id": snapshot.owner_agent_id,
            "pack_id": snapshot.pack_id,
            "task_profile": snapshot.task_profile,
            "lifecycle_mode": snapshot.lifecycle_mode,
            "primary_model": {
                "provider": snapshot.primary_provider,
                "model": snapshot.primary_model,
            },
            "local_route_hints": snapshot.local_route_hints,
            "task": snapshot.task,
            "service_state": snapshot.service_state.as_str(),
            "task_state": snapshot.task_state.as_str(),
            "started_at": snapshot.started_at.to_rfc3339(),
            "updated_at": snapshot.updated_at.to_rfc3339(),
            "completed_at": snapshot.completed_at.map(|at| at.to_rfc3339()),
            "container_name": snapshot.container_name,
            "container_id": snapshot.container_id,
            "config_dir": snapshot.config_dir.display().to_string(),
            "workspace_dir": snapshot.workspace_dir.display().to_string(),
            "runtime_state": runtime_state,
            "distributed_plan": distributed_plan,
            "docker_state": docker_state.as_ref().map(|state| json!({
                "container_id": state.container_id,
                "state": state.state,
            })),
            "progress": snapshot.progress,
            "result": snapshot.result.as_ref().map(|result| json!({
                "success": result.success,
                "output": result.output,
                "error": result.error,
            })),
            "last_error": snapshot.last_error,
        });
        if docker_state.is_none() {
            if let Some(object) = output.as_object_mut() {
                object.remove("docker_state");
            }
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&output)?,
            error: None,
        })
    }

    async fn handle_suspend(&self, agent_id: &str) -> anyhow::Result<ToolResult> {
        self.enforce_write("spawned_agent_manage:suspend")?;
        let Some(snapshot) = self.resolve_snapshot(agent_id)? else {
            return unknown_agent(agent_id);
        };
        if let Some(plan) = self.load_distributed_plan(&snapshot).await? {
            publish_json_message(
                &self.root_config.worker_plane,
                &plan.topics.command_topic,
                COMMAND_TYPE_SUSPEND_AGENT_REQUESTED,
                agent_id,
                &json!({
                    "event_id": uuid::Uuid::new_v4().to_string(),
                    "agent_id": agent_id,
                    "reason": "suspended by orchestrator",
                    "requested_at": Utc::now().to_rfc3339(),
                }),
            )
            .await?;
            if self.registry.exists(agent_id) {
                self.registry
                    .mark_service_state(agent_id, SpawnedAgentServiceState::Suspended);
            }
            return Ok(ToolResult {
                success: true,
                output: json!({
                    "agent_id": agent_id,
                    "service_state": "suspended",
                    "delivery_backend": "redpanda_k8s",
                })
                .to_string(),
                error: None,
            });
        }

        self.spawner.stop_service(&snapshot.container_name).await?;
        if self.registry.exists(agent_id) {
            self.registry
                .mark_service_state(agent_id, SpawnedAgentServiceState::Suspended);
        }
        Ok(ToolResult {
            success: true,
            output: json!({
                "agent_id": agent_id,
                "service_state": "suspended",
            })
            .to_string(),
            error: None,
        })
    }

    async fn handle_resume(&self, agent_id: &str) -> anyhow::Result<ToolResult> {
        self.enforce_write("spawned_agent_manage:resume")?;
        let Some(snapshot) = self.resolve_snapshot(agent_id)? else {
            return unknown_agent(agent_id);
        };
        if let Some(plan) = self.load_distributed_plan(&snapshot).await? {
            publish_json_message(
                &self.root_config.worker_plane,
                &plan.topics.command_topic,
                COMMAND_TYPE_RESUME_AGENT_REQUESTED,
                agent_id,
                &json!({
                    "event_id": uuid::Uuid::new_v4().to_string(),
                    "agent_id": agent_id,
                    "requested_at": Utc::now().to_rfc3339(),
                }),
            )
            .await?;
            if self.registry.exists(agent_id) {
                self.registry
                    .mark_service_state(agent_id, SpawnedAgentServiceState::Running);
            }
            return Ok(ToolResult {
                success: true,
                output: json!({
                    "agent_id": agent_id,
                    "service_state": "running",
                    "delivery_backend": "redpanda_k8s",
                })
                .to_string(),
                error: None,
            });
        }

        self.spawner.start_service(&snapshot.container_name).await?;
        if self.registry.exists(agent_id) {
            self.registry
                .mark_service_state(agent_id, SpawnedAgentServiceState::Running);
        }
        Ok(ToolResult {
            success: true,
            output: json!({
                "agent_id": agent_id,
                "service_state": "running",
            })
            .to_string(),
            error: None,
        })
    }

    async fn handle_terminate(&self, agent_id: &str) -> anyhow::Result<ToolResult> {
        self.enforce_write("spawned_agent_manage:terminate")?;
        let Some(snapshot) = self.resolve_snapshot(agent_id)? else {
            return unknown_agent(agent_id);
        };
        if let Some(plan) = self.load_distributed_plan(&snapshot).await? {
            publish_json_message(
                &self.root_config.worker_plane,
                &plan.topics.command_topic,
                COMMAND_TYPE_TERMINATE_AGENT_REQUESTED,
                agent_id,
                &json!({
                    "event_id": uuid::Uuid::new_v4().to_string(),
                    "agent_id": agent_id,
                    "reason": "terminated by orchestrator",
                    "requested_at": Utc::now().to_rfc3339(),
                }),
            )
            .await?;
            if self.registry.exists(agent_id) {
                self.registry
                    .mark_service_state(agent_id, SpawnedAgentServiceState::Terminated);
                if matches!(
                    snapshot.task_state,
                    SpawnedAgentTaskState::Running | SpawnedAgentTaskState::Pending
                ) {
                    self.registry.cancel_task(
                        agent_id,
                        "Spawned agent termination requested by orchestrator".into(),
                    );
                }
            }
            return Ok(ToolResult {
                success: true,
                output: json!({
                    "agent_id": agent_id,
                    "service_state": "terminated",
                    "delivery_backend": "redpanda_k8s",
                })
                .to_string(),
                error: None,
            });
        }

        self.spawner
            .terminate_service(&snapshot.container_name)
            .await?;
        if self.registry.exists(agent_id) {
            self.registry
                .mark_service_state(agent_id, SpawnedAgentServiceState::Terminated);
            if matches!(
                snapshot.task_state,
                SpawnedAgentTaskState::Running | SpawnedAgentTaskState::Pending
            ) {
                self.registry
                    .cancel_task(agent_id, "Spawned agent terminated by orchestrator".into());
            }
        }

        Ok(ToolResult {
            success: true,
            output: json!({
                "agent_id": agent_id,
                "service_state": "terminated",
            })
            .to_string(),
            error: None,
        })
    }

    fn enforce_write(&self, tool_name: &str) -> anyhow::Result<()> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, tool_name)
        {
            anyhow::bail!(error);
        }
        Ok(())
    }

    fn resolve_snapshot(&self, agent_id: &str) -> anyhow::Result<Option<SpawnedAgentStatusSnapshot>> {
        if let Some(snapshot) = self.registry.get_status(agent_id) {
            return Ok(Some(snapshot));
        }
        discover_status_snapshot(&self.labaclaw_dir, agent_id)
    }

    async fn load_distributed_plan(
        &self,
        snapshot: &SpawnedAgentStatusSnapshot,
    ) -> anyhow::Result<Option<DistributedSpawnPlan>> {
        let path = distributed_spawn_plan_path(&snapshot.config_dir);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(anyhow::Error::from)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let plan: DistributedSpawnPlan = serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        if plan
            .delivery_backend
            .trim()
            .eq_ignore_ascii_case("redpanda_k8s")
        {
            Ok(Some(plan))
        } else {
            Ok(None)
        }
    }
}

fn unknown_agent(agent_id: &str) -> anyhow::Result<ToolResult> {
    Ok(ToolResult {
        success: false,
        output: String::new(),
        error: Some(format!("Unknown spawned agent '{agent_id}'")),
    })
}
