//! Dedicated agent provisioning tool.
//!
//! Implements the `spawn_agent` tool that turns a high-level task into a
//! versioned `AgentSpec`, materialized profile, and isolated Docker-backed
//! child agent instance owned by the orchestrator.

use super::spawned_agent_registry::{
    SpawnedAgentProgressEntry, SpawnedAgentRegistry, SpawnedAgentServiceState, SpawnedAgentSession,
    SpawnedAgentTaskState,
};
use super::traits::{Tool, ToolResult};
use crate::agent::classifier;
use crate::config::{resolve_default_model_id, AgentLoadBalanceStrategy, Config, ModelRouteConfig};
use crate::providers;
use crate::runtime::{AgentWorkspaceMount, DockerAgentSpawnRequest, DockerAgentSpawner};
use crate::security::policy::ToolOperation;
use crate::security::{AutonomyLevel, SecurityPolicy};
use crate::spawned_runtime::{
    bootstrap_result_path, read_runtime_state, runtime_state_path, write_task_request,
    SpawnedAgentRuntimeState, SpawnedAgentTaskRequest,
};
use crate::worker_plane::{
    build_distributed_spawn_plan, build_local_artifact_refs, build_spawn_plan_from_refs,
    local_file_ref, publish_json_message, spawn_request_payload, summarize_text_for_event,
    upload_bytes_to_artifact_ref, worker_plane_enabled, write_distributed_spawn_plan,
    COMMAND_TYPE_SPAWN_AGENT_REQUESTED,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

const OWNER_AGENT_ID: &str = "orchestrator";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TaskProfile {
    FastConversational,
    DeepReasoning,
    DocumentAnalysis,
    VisionAnalysis,
    StructuredExtraction,
}

impl TaskProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::FastConversational => "fast_conversational",
            Self::DeepReasoning => "deep_reasoning",
            Self::DocumentAnalysis => "document_analysis",
            Self::VisionAnalysis => "vision_analysis",
            Self::StructuredExtraction => "structured_extraction",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fast_conversational" | "fast" => Some(Self::FastConversational),
            "deep_reasoning" | "reasoning" => Some(Self::DeepReasoning),
            "document_analysis" | "document" => Some(Self::DocumentAnalysis),
            "vision_analysis" | "vision" => Some(Self::VisionAnalysis),
            "structured_extraction" | "structured" | "extract" => Some(Self::StructuredExtraction),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LifecycleMode {
    Dedicated,
    Ephemeral,
}

impl LifecycleMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dedicated => "dedicated",
            Self::Ephemeral => "ephemeral",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dedicated" => Some(Self::Dedicated),
            "ephemeral" => Some(Self::Ephemeral),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReasoningPolicy {
    Minimal,
    Standard,
    Deep,
}

impl ReasoningPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Deep => "deep",
        }
    }
}

#[derive(Debug, Clone)]
struct ModelCandidate {
    provider: String,
    model: String,
    api_key: Option<String>,
    hint: Option<String>,
    transport: Option<String>,
    max_tokens: Option<u32>,
    reasoning_score: u8,
    context_score: u8,
    structured_output_score: u8,
    document_score: u8,
    vision: bool,
    cost_tier: u8,
    latency_tier: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelBinding {
    hint: Option<String>,
    provider: String,
    model: String,
    transport: Option<String>,
    max_tokens: Option<u32>,
    selection_reason: String,
}

#[derive(Debug, Clone)]
struct SelectedLocalRoute {
    hint: String,
    candidate: ModelCandidate,
    selection_reason: String,
}

#[derive(Debug, Clone)]
struct SelectedModelPlan {
    task_profile: TaskProfile,
    reasoning_policy: ReasoningPolicy,
    primary: ModelCandidate,
    local_routes: Vec<SelectedLocalRoute>,
    rationale: String,
}

#[derive(Debug, Clone)]
struct SpawnWorkflowRequest {
    agent_id: String,
    spec: AgentSpec,
    selected_models: SelectedModelPlan,
    image: String,
    container_name: String,
    workspace_mounts: Vec<AgentWorkspaceMount>,
    boot_message: String,
    lifecycle_mode: LifecycleMode,
    task: String,
}

struct AgentSpecBuildRequest<'a> {
    agent_id: &'a str,
    display_name: &'a str,
    pack: &'a PackProfile,
    task_profile: TaskProfile,
    lifecycle_mode: LifecycleMode,
    image: String,
    workspace_mounts: &'a [AgentWorkspaceMountSpec],
    selected_models: &'a SelectedModelPlan,
    initial_mission: String,
    network: String,
    memory_limit_mb: Option<u64>,
    cpu_limit: Option<f64>,
    read_only_rootfs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentPersonaSpec {
    role: String,
    mission: String,
    operating_style: Vec<String>,
    responsibilities: Vec<String>,
    prohibitions: Vec<String>,
    direct_user: String,
    indirect_user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentToolPolicySpec {
    allowed_tools: Vec<String>,
    denied_tools: Vec<String>,
    allowed_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentRuntimeSpec {
    kind: String,
    image: String,
    network: String,
    memory_limit_mb: Option<u64>,
    cpu_limit: Option<f64>,
    read_only_rootfs: bool,
    daemon_host: String,
    daemon_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentWorkspaceMountSpec {
    host_path: String,
    container_path: String,
    read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentMemoryScopeSpec {
    strategy: String,
    retain_outputs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentArtifactPolicySpec {
    spec_dir: String,
    events_dir: String,
    workspace_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentRoutingSpec {
    command_stream: String,
    event_stream: String,
    heartbeat_stream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSpec {
    schema_version: String,
    agent_id: String,
    display_name: String,
    owner_agent_id: String,
    upstream_user_kind: String,
    pack_id: String,
    persona: AgentPersonaSpec,
    capabilities: Vec<String>,
    tool_policy: AgentToolPolicySpec,
    runtime: AgentRuntimeSpec,
    workspace_mounts: Vec<AgentWorkspaceMountSpec>,
    memory_scope: AgentMemoryScopeSpec,
    artifact_policy: AgentArtifactPolicySpec,
    routing: AgentRoutingSpec,
    lifecycle_mode: String,
    task_profile: String,
    primary_llm: ModelBinding,
    local_model_routes: Vec<ModelBinding>,
    reasoning_policy: String,
    model_selection_rationale: String,
    initial_mission: String,
    created_at: String,
}

#[derive(Debug, Clone)]
struct PackProfile {
    pack_id: String,
    role: String,
    operating_style: Vec<String>,
    responsibilities: Vec<String>,
    prohibitions: Vec<String>,
    capabilities: Vec<String>,
    allowed_tools: Vec<String>,
    allowed_commands: Vec<String>,
    compact_context: bool,
}

#[derive(Debug, Clone)]
struct MaterializedAgentPaths {
    agent_home: PathBuf,
    workspace_dir: PathBuf,
    config_path: PathBuf,
    spec_dir: PathBuf,
    spec_version: String,
    specs_json_path: PathBuf,
    runtime_state_path: PathBuf,
    bootstrap_result_path: PathBuf,
}

#[derive(Clone)]
pub struct SpawnAgentTool {
    root_config: Arc<Config>,
    security: Arc<SecurityPolicy>,
    registry: Arc<SpawnedAgentRegistry>,
    labaclaw_dir: PathBuf,
    spawner: DockerAgentSpawner,
}

impl SpawnAgentTool {
    pub fn new(
        root_config: Arc<Config>,
        security: Arc<SecurityPolicy>,
        registry: Arc<SpawnedAgentRegistry>,
        labaclaw_dir: PathBuf,
        spawner: DockerAgentSpawner,
    ) -> Self {
        Self {
            root_config,
            security,
            registry,
            labaclaw_dir,
            spawner,
        }
    }
}

#[derive(Debug, Clone)]
struct SpawnAgentRequest {
    agent_name: String,
    task: String,
    context: String,
    pack_id: Option<String>,
    capabilities: Vec<String>,
    task_profile: Option<TaskProfile>,
    lifecycle_mode: LifecycleMode,
    image: Option<String>,
    workspace_mounts: Vec<PathBuf>,
    workspace_write_access: bool,
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Create a dedicated child agent owned by the orchestrator. The tool drafts an AgentSpec, \
         selects a model from the orchestrator inventory, materializes the child profile, starts \
         a Docker-isolated service instance, and runs the initial mission asynchronously."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "agent_name": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Display name for the dedicated child agent"
                },
                "task": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Mission that the spawned agent should execute first"
                },
                "context": {
                    "type": "string",
                    "description": "Optional supplemental context from the orchestrator"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Optional pack/persona preset. Examples: financial_analyst, document_specialist, software_architect, general_specialist"
                },
                "capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional extra capability tags for the agent spec"
                },
                "task_profile": {
                    "type": "string",
                    "enum": [
                        "fast_conversational",
                        "deep_reasoning",
                        "document_analysis",
                        "vision_analysis",
                        "structured_extraction"
                    ],
                    "description": "Optional override for task classification"
                },
                "lifecycle_mode": {
                    "type": "string",
                    "enum": ["dedicated", "ephemeral"],
                    "description": "Whether to keep the service running after the initial mission"
                },
                "image": {
                    "type": "string",
                    "description": "Optional Docker image override. Defaults to runtime.docker.image"
                },
                "workspace_mounts": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional host paths mounted into the spawned agent container"
                },
                "workspace_write_access": {
                    "type": "boolean",
                    "description": "If true, mounted workspaces are read-write instead of read-only"
                }
            },
            "required": ["agent_name", "task"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "spawn_agent")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let request = parse_request(&args, &self.root_config.workspace_dir)?;
        let task_profile = request.task_profile.unwrap_or_else(|| {
            classify_task_profile(&self.root_config, &request.task, &request.context)
        });
        let pack = derive_pack_profile(
            request.pack_id.as_deref(),
            &request.task,
            &request.context,
            task_profile,
            &request.capabilities,
        );
        let selected_models = select_models(&self.root_config, task_profile)?;
        let agent_id = format!(
            "{}-{}",
            slugify(&request.agent_name),
            &Uuid::new_v4().simple().to_string()[..12]
        );
        let distributed_backend = worker_plane_enabled(&self.root_config.worker_plane);
        let image = resolve_spawn_image(
            request.image.as_deref(),
            &self.root_config,
            distributed_backend,
        )?;
        let container_name = format!("labaclaw-agent-{agent_id}");

        let workspace_mounts = request
            .workspace_mounts
            .iter()
            .enumerate()
            .map(|(index, host_path)| AgentWorkspaceMountSpec {
                host_path: host_path.display().to_string(),
                container_path: format!("/mounted-workspaces/{index}"),
                read_only: !request.workspace_write_access,
            })
            .collect::<Vec<_>>();

        let initial_mission = build_initial_mission(
            &request.task,
            &request.context,
            &workspace_mounts,
            task_profile,
        );

        let spec = build_agent_spec(AgentSpecBuildRequest {
            agent_id: &agent_id,
            display_name: &request.agent_name,
            pack: &pack,
            task_profile,
            lifecycle_mode: request.lifecycle_mode,
            image: image.clone(),
            workspace_mounts: &workspace_mounts,
            selected_models: &selected_models,
            initial_mission: initial_mission.clone(),
            network: self.root_config.runtime.docker.network.clone(),
            memory_limit_mb: self.root_config.runtime.docker.memory_limit_mb,
            cpu_limit: self.root_config.runtime.docker.cpu_limit,
            read_only_rootfs: self.root_config.runtime.docker.read_only_rootfs,
        });

        let agent_home = self.labaclaw_dir.join("spawned-agents").join(&agent_id);
        let runtime_state_path = runtime_state_path(&agent_home);

        let session = SpawnedAgentSession {
            agent_id: agent_id.clone(),
            display_name: request.agent_name.clone(),
            owner_agent_id: OWNER_AGENT_ID.to_string(),
            pack_id: spec.pack_id.clone(),
            task_profile: spec.task_profile.clone(),
            lifecycle_mode: spec.lifecycle_mode.clone(),
            primary_provider: spec.primary_llm.provider.clone(),
            primary_model: spec.primary_llm.model.clone(),
            local_route_hints: spec
                .local_model_routes
                .iter()
                .filter_map(|route| route.hint.clone())
                .collect(),
            task: request.task.clone(),
            config_dir: agent_home.clone(),
            workspace_dir: agent_home.join("workspace"),
            runtime_state_path: runtime_state_path.clone(),
            container_name: container_name.clone(),
            container_id: None,
            service_state: SpawnedAgentServiceState::Provisioning,
            task_state: SpawnedAgentTaskState::Pending,
            started_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            result: None,
            last_error: None,
            progress: vec![SpawnedAgentProgressEntry {
                at: Utc::now().to_rfc3339(),
                stage: "queued".into(),
                detail: "Agent provisioning queued by orchestrator".into(),
            }],
            handle: None,
        };
        self.registry.insert(session);

        let registry = self.registry.clone();
        let spawner = self.spawner.clone();
        let root_config = self.root_config.clone();
        let boot_message = initial_mission;
        let workflow_agent_id = agent_id.clone();
        let workflow_spec = spec.clone();
        let request_workspace_mounts = request
            .workspace_mounts
            .iter()
            .enumerate()
            .map(|(index, host_path)| AgentWorkspaceMount {
                host_path: host_path.clone(),
                container_path: format!("/mounted-workspaces/{index}"),
                read_only: !request.workspace_write_access,
            })
            .collect::<Vec<_>>();
        let lifecycle_mode = request.lifecycle_mode;
        let task = request.task.clone();

        let handle = tokio::spawn(async move {
            let workflow_request = SpawnWorkflowRequest {
                agent_id: workflow_agent_id.clone(),
                spec: workflow_spec,
                selected_models,
                image,
                container_name,
                workspace_mounts: request_workspace_mounts,
                boot_message,
                lifecycle_mode,
                task,
            };
            if let Err(error) =
                run_spawn_workflow(&registry, &spawner, root_config.as_ref(), workflow_request)
                    .await
            {
                registry.append_progress(
                    &workflow_agent_id,
                    "failed",
                    format!("Provisioning failed: {error}"),
                );
                registry.mark_service_state(&workflow_agent_id, SpawnedAgentServiceState::Failed);
                registry.fail_task(&workflow_agent_id, error.to_string());
            }
        });
        self.registry.set_handle(&agent_id, handle);

        let output = json!({
            "agent_id": agent_id,
            "status": "provisioning",
            "message": "Dedicated specialist is being created. Use spawned_agent_manage for live status.",
            "delivery_backend": if distributed_backend {
                "redpanda_k8s"
            } else {
                "local_docker"
            },
            "pack_id": spec.pack_id,
            "task_profile": spec.task_profile,
            "lifecycle_mode": spec.lifecycle_mode,
            "selected_primary_model": {
                "provider": spec.primary_llm.provider,
                "model": spec.primary_llm.model,
            },
            "selected_routes": spec.local_model_routes,
        });

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&output)?,
            error: None,
        })
    }
}

async fn run_spawn_workflow(
    registry: &SpawnedAgentRegistry,
    spawner: &DockerAgentSpawner,
    root_config: &Config,
    request: SpawnWorkflowRequest,
) -> Result<()> {
    let SpawnWorkflowRequest {
        agent_id,
        spec,
        selected_models,
        image,
        container_name,
        workspace_mounts,
        boot_message,
        lifecycle_mode,
        task,
    } = request;
    let distributed_backend = worker_plane_enabled(&root_config.worker_plane);
    registry.append_progress(&agent_id, "drafted", "AgentSpec drafted");
    let materialized = materialize_agent(
        root_config,
        &spec,
        &selected_models,
        workspace_mounts.as_slice(),
    )
    .await?;
    write_event(
        &materialized.agent_home,
        "AgentSpecDrafted",
        json!({
            "event_id": Uuid::new_v4().to_string(),
            "agent_id": spec.agent_id,
            "owner_agent_id": spec.owner_agent_id,
            "pack_id": spec.pack_id,
            "task_profile": spec.task_profile,
            "created_at": spec.created_at,
        }),
    )
    .await?;

    write_event(
        &materialized.agent_home,
        "AgentModelSelected",
        json!({
            "event_id": Uuid::new_v4().to_string(),
            "agent_id": spec.agent_id,
            "task_profile": spec.task_profile,
            "selected_primary_model": {
                "provider": spec.primary_llm.provider,
                "model": spec.primary_llm.model,
            },
            "selected_routes": spec.local_model_routes,
            "selection_basis": "capability_cost_latency",
        }),
    )
    .await?;

    write_event(
        &materialized.agent_home,
        "AgentSpecVersioned",
        json!({
            "event_id": Uuid::new_v4().to_string(),
            "agent_id": spec.agent_id,
            "spec_ref": if distributed_backend {
                build_distributed_spawn_plan(
                    &root_config.worker_plane,
                    &spec.agent_id,
                    &spec.owner_agent_id,
                    &spec.lifecycle_mode,
                    &spec.task_profile,
                    &materialized.spec_version,
                    "draft",
                ).artifact_refs.spec_ref
            } else {
                local_file_ref(&materialized.specs_json_path)
            },
            "version_dir": materialized.spec_dir.display().to_string(),
        }),
    )
    .await?;

    registry.append_progress(
        &agent_id,
        "materialized",
        format!("Profile rendered into {}", materialized.spec_dir.display()),
    );

    let boot_request = SpawnedAgentTaskRequest {
        request_id: Uuid::new_v4().to_string(),
        message: boot_message.clone(),
        max_history_messages: Some(16),
        max_tool_iterations: Some(root_config.agent.max_tool_iterations.min(12)),
        compact_context: matches!(spec.reasoning_policy.as_str(), "minimal" | "standard")
            && root_config.agent.compact_context,
        created_at: Utc::now().to_rfc3339(),
    };
    let boot_request_path = write_task_request(&materialized.agent_home, &boot_request).await?;
    let distributed_artifact_refs = if distributed_backend {
        build_distributed_spawn_plan(
            &root_config.worker_plane,
            &spec.agent_id,
            &spec.owner_agent_id,
            &spec.lifecycle_mode,
            &spec.task_profile,
            &materialized.spec_version,
            &boot_request.request_id,
        )
        .artifact_refs
    } else {
        build_local_artifact_refs(
            &materialized.specs_json_path,
            &boot_request_path,
            &materialized.bootstrap_result_path,
            &materialized.agent_home.join("artifacts"),
            &materialized.agent_home.join("questions"),
        )
    };
    let local_plan = build_spawn_plan_from_refs(
        &root_config.worker_plane,
        &spec.agent_id,
        &spec.owner_agent_id,
        &spec.lifecycle_mode,
        &spec.task_profile,
        distributed_artifact_refs.clone(),
    );
    let distributed_plan = build_distributed_spawn_plan(
        &root_config.worker_plane,
        &spec.agent_id,
        &spec.owner_agent_id,
        &spec.lifecycle_mode,
        &spec.task_profile,
        &materialized.spec_version,
        &boot_request.request_id,
    );
    let spawn_plan = if distributed_backend {
        distributed_plan
    } else {
        local_plan
    };
    let spawn_plan_path =
        write_distributed_spawn_plan(&materialized.agent_home, &spawn_plan).await?;
    registry.append_progress(
        &agent_id,
        "bootstrap_queued",
        "Initial mission persisted for container runtime",
    );

    let spawn_request = DockerAgentSpawnRequest {
        container_name: container_name.to_string(),
        image: image.to_string(),
        host_config_dir: materialized.agent_home.clone(),
        container_config_dir: "/agent".into(),
        workspace_mounts,
        env: HashMap::new(),
        labels: HashMap::from([
            (String::from("labaclaw.agent_id"), spec.agent_id.clone()),
            (
                String::from("labaclaw.owner_agent_id"),
                spec.owner_agent_id.clone(),
            ),
            (String::from("labaclaw.pack_id"), spec.pack_id.clone()),
        ]),
    };

    write_event(
        &materialized.agent_home,
        "AgentSpawnRequested",
        json!({
            "event_id": spawn_plan.spawn_command.event_id.clone(),
            "agent_id": spawn_plan.spawn_command.agent_id.clone(),
            "owner_agent_id": spawn_plan.spawn_command.owner_agent_id.clone(),
            "spec_ref": spawn_plan.spawn_command.spec_ref.clone(),
            "bootstrap_ref": spawn_plan.spawn_command.bootstrap_ref.clone(),
            "lifecycle_mode": spawn_plan.spawn_command.lifecycle_mode.clone(),
            "task_profile": spawn_plan.spawn_command.task_profile.clone(),
            "requested_at": spawn_plan.spawn_command.requested_at.clone(),
            "delivery_backend": spawn_plan.delivery_backend.clone(),
            "worker_namespace": spawn_plan.worker_namespace.clone(),
            "plan_path": spawn_plan_path.display().to_string(),
            "container_name": container_name,
            "image": image,
        }),
    )
    .await?;
    if distributed_backend {
        let spec_bytes = fs::read(&materialized.specs_json_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to read AgentSpec payload from {}",
                    materialized.specs_json_path.display()
                )
            })?;
        upload_bytes_to_artifact_ref(
            &root_config.worker_plane,
            &spawn_plan.artifact_refs.spec_ref,
            spec_bytes,
        )
        .await?;
        registry.append_progress(
            &agent_id,
            "spec_uploaded",
            format!(
                "Uploaded AgentSpec to {}",
                spawn_plan.artifact_refs.spec_ref
            ),
        );

        let bootstrap_bytes = fs::read(&boot_request_path).await.with_context(|| {
            format!(
                "Failed to read bootstrap request from {}",
                boot_request_path.display()
            )
        })?;
        upload_bytes_to_artifact_ref(
            &root_config.worker_plane,
            &spawn_plan.artifact_refs.bootstrap_ref,
            bootstrap_bytes,
        )
        .await?;
        registry.append_progress(
            &agent_id,
            "bootstrap_uploaded",
            format!(
                "Uploaded bootstrap request to {}",
                spawn_plan.artifact_refs.bootstrap_ref
            ),
        );

        publish_json_message(
            &root_config.worker_plane,
            &spawn_plan.topics.command_topic,
            COMMAND_TYPE_SPAWN_AGENT_REQUESTED,
            &spawn_plan.spawn_command.agent_id,
            &spawn_request_payload(&spawn_plan),
        )
        .await?;
        registry.append_progress(
            &agent_id,
            "published",
            format!(
                "Spawn plan staged for worker-plane on topics {} / {}",
                spawn_plan.topics.command_topic, spawn_plan.topics.event_topic
            ),
        );
        registry.clear_handle(&agent_id);
        return Ok(());
    }

    registry.append_progress(&agent_id, "container_start", "Starting Docker service");

    let container_id = spawner.spawn_service(&spawn_request).await?;
    registry.mark_service_running(&agent_id, container_id.clone());
    registry.append_progress(&agent_id, "container_running", "Docker service is running");
    write_event(
        &materialized.agent_home,
        "AgentSpawned",
        json!({
            "event_id": Uuid::new_v4().to_string(),
            "agent_id": spec.agent_id,
            "runtime_backend": "local_docker",
            "workload_kind": "Container",
            "workload_namespace": "docker",
            "workload_name": container_name,
            "container_id": container_id,
            "container_name": container_name,
        }),
    )
    .await?;

    registry.mark_task_running(&agent_id);
    registry.append_progress(
        &agent_id,
        "bootstrap_task",
        "Waiting for initial mission result",
    );

    match wait_for_initial_mission(
        spawner,
        &container_name,
        &materialized.runtime_state_path,
        &materialized.bootstrap_result_path,
        registry,
        &agent_id,
    )
    .await
    {
        Ok(output) => {
            registry.append_progress(&agent_id, "completed", "Initial mission completed");
            let result = ToolResult {
                success: true,
                output: output.clone(),
                error: None,
            };
            registry.complete_task(&agent_id, result);
            write_event(
                &materialized.agent_home,
                "AgentCompleted",
                json!({
                    "event_id": Uuid::new_v4().to_string(),
                    "agent_id": spec.agent_id,
                    "request_id": boot_request.request_id,
                    "task": task,
                    "result_ref": local_file_ref(&materialized.bootstrap_result_path),
                    "summary": summarize_text_for_event(&output),
                }),
            )
            .await?;

            if matches!(lifecycle_mode, LifecycleMode::Ephemeral) {
                spawner.terminate_service(&container_name).await?;
                registry.mark_service_state(&agent_id, SpawnedAgentServiceState::Terminated);
                write_event(
                    &materialized.agent_home,
                    "AgentTerminated",
                    json!({
                        "event_id": Uuid::new_v4().to_string(),
                        "agent_id": spec.agent_id,
                        "reason": "ephemeral lifecycle completed",
                    }),
                )
                .await?;
            }
        }
        Err(error) => {
            registry.append_progress(
                &agent_id,
                "task_failed",
                format!("Initial mission failed: {error}"),
            );
            registry.fail_task(&agent_id, error.to_string());
            write_event(
                &materialized.agent_home,
                "AgentSpawnFailed",
                json!({
                    "event_id": Uuid::new_v4().to_string(),
                    "agent_id": spec.agent_id,
                    "request_id": boot_request.request_id,
                    "error": error.to_string(),
                    "failed_at": Utc::now().to_rfc3339(),
                }),
            )
            .await?;
        }
    }

    Ok(())
}

fn parse_request(args: &serde_json::Value, default_workspace: &Path) -> Result<SpawnAgentRequest> {
    let agent_name = args
        .get("agent_name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing or empty 'agent_name' parameter"))?
        .to_string();
    let task = args
        .get("task")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing or empty 'task' parameter"))?
        .to_string();
    let context = args
        .get("context")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let pack_id = args
        .get("pack_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let capabilities = args
        .get("capabilities")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let task_profile = args
        .get("task_profile")
        .and_then(|value| value.as_str())
        .and_then(TaskProfile::from_str);
    let lifecycle_mode = args
        .get("lifecycle_mode")
        .and_then(|value| value.as_str())
        .and_then(LifecycleMode::from_str)
        .unwrap_or(LifecycleMode::Dedicated);
    let image = args
        .get("image")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let workspace_mounts = args
        .get("workspace_mounts")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .filter(|paths| !paths.is_empty())
        .unwrap_or_else(|| vec![default_workspace.to_path_buf()]);
    let workspace_write_access = args
        .get("workspace_write_access")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    Ok(SpawnAgentRequest {
        agent_name,
        task,
        context,
        pack_id,
        capabilities,
        task_profile,
        lifecycle_mode,
        image,
        workspace_mounts,
        workspace_write_access,
    })
}

fn resolve_spawn_image(
    image_override: Option<&str>,
    config: &Config,
    distributed_backend: bool,
) -> Result<String> {
    let image = image_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !is_placeholder_spawn_image(value))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| config.runtime.docker.image.trim().to_string());

    if image.is_empty() {
        anyhow::bail!("Spawned agent image is empty. Set runtime.docker.image or pass image.");
    }

    if distributed_backend && image == "alpine:3.20" {
        return Ok("worker-plane-managed".into());
    }

    if image == "alpine:3.20" {
        anyhow::bail!(
            "runtime.docker.image is still the default alpine image. Configure a LabaClaw image before using spawn_agent."
        );
    }

    Ok(image)
}

fn is_placeholder_spawn_image(image: &str) -> bool {
    matches!(
        image.trim().to_ascii_lowercase().as_str(),
        "example:latest" | "example" | "<image>" | "your-image:tag"
    )
}

fn classify_task_profile(config: &Config, task: &str, context: &str) -> TaskProfile {
    let joined = format!("{task}\n{context}");
    if let Some(hint) = classifier::classify(&config.query_classification, &joined) {
        let lowered = hint.to_ascii_lowercase();
        if lowered.contains("vision") {
            return TaskProfile::VisionAnalysis;
        }
        if lowered.contains("reason") {
            return TaskProfile::DeepReasoning;
        }
        if lowered.contains("document") || lowered.contains("doc") {
            return TaskProfile::DocumentAnalysis;
        }
        if lowered.contains("extract") || lowered.contains("json") {
            return TaskProfile::StructuredExtraction;
        }
        if lowered.contains("fast") {
            return TaskProfile::FastConversational;
        }
    }

    let lowered = joined.to_ascii_lowercase();
    if contains_any(
        &lowered,
        &["image", "scan", "screenshot", "photo", "ocr", "vision"],
    ) {
        return TaskProfile::VisionAnalysis;
    }
    if contains_any(
        &lowered,
        &[
            "invoice",
            "faktura",
            "ledger",
            "spreadsheet",
            "csv",
            "xlsx",
            "statement",
            "document",
            "pdf",
        ],
    ) {
        if contains_any(
            &lowered,
            &[
                "cause",
                "why",
                "root cause",
                "margin",
                "variance",
                "trend",
                "recommend",
            ],
        ) {
            return TaskProfile::DeepReasoning;
        }
        return TaskProfile::DocumentAnalysis;
    }
    if contains_any(
        &lowered,
        &["extract", "schema", "json", "table", "fields", "normalize"],
    ) {
        return TaskProfile::StructuredExtraction;
    }
    if lowered.len() > 600
        || contains_any(
            &lowered,
            &[
                "analyze",
                "investigate",
                "trade-off",
                "architecture",
                "strategy",
                "plan",
            ],
        )
    {
        return TaskProfile::DeepReasoning;
    }
    TaskProfile::FastConversational
}

fn derive_pack_profile(
    explicit_pack_id: Option<&str>,
    task: &str,
    context: &str,
    task_profile: TaskProfile,
    requested_capabilities: &[String],
) -> PackProfile {
    let pack_id = explicit_pack_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let lowered = format!("{task}\n{context}").to_ascii_lowercase();
            if contains_any(
                &lowered,
                &[
                    "invoice", "faktura", "cost", "margin", "profit", "expense", "finance",
                ],
            ) {
                "financial_analyst".to_string()
            } else if contains_any(
                &lowered,
                &["pdf", "document", "contract", "brief", "extract", "report"],
            ) {
                "document_specialist".to_string()
            } else if contains_any(
                &lowered,
                &[
                    "code", "repo", "service", "api", "bug", "refactor", "software",
                ],
            ) {
                "software_architect".to_string()
            } else {
                "general_specialist".to_string()
            }
        });

    let mut profile = match pack_id.as_str() {
        "financial_analyst" => PackProfile {
            pack_id,
            role: "Financial Analysis Specialist".into(),
            operating_style: vec![
                "Be explicit about assumptions and data quality.".into(),
                "Prioritize financial causality, variance explanation, and decision support."
                    .into(),
            ],
            responsibilities: vec![
                "Inspect financial files and supporting documents.".into(),
                "Explain trends, anomalies, and root causes.".into(),
                "Return a structured recommendation for the orchestrator.".into(),
            ],
            prohibitions: vec![
                "Do not contact the human directly.".into(),
                "Do not invent missing figures or ledger entries.".into(),
                "Do not create or spawn additional agents.".into(),
            ],
            capabilities: vec![
                "investigate".into(),
                "document_analysis".into(),
                "financial_reasoning".into(),
                "structured_reporting".into(),
            ],
            allowed_tools: vec![
                "task_plan".into(),
                "file_read".into(),
                "glob_search".into(),
                "content_search".into(),
                "pdf_read".into(),
                "xlsx_read".into(),
                "docx_read".into(),
                "pptx_read".into(),
                "http_request".into(),
                "web_fetch".into(),
                "memory_observe".into(),
                "memory_store".into(),
                "memory_recall".into(),
            ],
            allowed_commands: Vec::new(),
            compact_context: false,
        },
        "document_specialist" => PackProfile {
            pack_id,
            role: "Document Analysis Specialist".into(),
            operating_style: vec![
                "Read thoroughly before concluding.".into(),
                "Separate extraction from interpretation.".into(),
            ],
            responsibilities: vec![
                "Extract facts from documents and files.".into(),
                "Organize findings in a reusable structure.".into(),
            ],
            prohibitions: vec![
                "Do not contact the human directly.".into(),
                "Do not fabricate missing clauses or figures.".into(),
                "Do not create or spawn additional agents.".into(),
            ],
            capabilities: vec![
                "document_analysis".into(),
                "structured_extraction".into(),
                "summarization".into(),
            ],
            allowed_tools: vec![
                "task_plan".into(),
                "file_read".into(),
                "glob_search".into(),
                "content_search".into(),
                "pdf_read".into(),
                "xlsx_read".into(),
                "docx_read".into(),
                "pptx_read".into(),
                "memory_observe".into(),
                "memory_store".into(),
            ],
            allowed_commands: Vec::new(),
            compact_context: false,
        },
        "software_architect" => PackProfile {
            pack_id,
            role: "Software Architecture Specialist".into(),
            operating_style: vec![
                "Work from architecture and constraints toward code changes.".into(),
                "Prefer clear design trade-offs over ad hoc patches.".into(),
            ],
            responsibilities: vec![
                "Inspect codebases and propose implementation direction.".into(),
                "Deliver actionable architecture and code-oriented findings.".into(),
            ],
            prohibitions: vec![
                "Do not contact the human directly.".into(),
                "Do not create or spawn additional agents.".into(),
                "Do not rewrite unrelated code.".into(),
            ],
            capabilities: vec![
                "software_analysis".into(),
                "implementation_planning".into(),
                "technical_reasoning".into(),
            ],
            allowed_tools: vec![
                "task_plan".into(),
                "file_read".into(),
                "glob_search".into(),
                "content_search".into(),
                "git_operations".into(),
                "shell".into(),
                "memory_observe".into(),
                "memory_store".into(),
            ],
            allowed_commands: vec![
                "git".into(),
                "rg".into(),
                "cargo".into(),
                "npm".into(),
                "pnpm".into(),
                "python3".into(),
                "pytest".into(),
            ],
            compact_context: false,
        },
        _ => PackProfile {
            pack_id,
            role: "Dedicated Specialist".into(),
            operating_style: vec![
                "Stay focused on the assigned mission.".into(),
                "Escalate blockers back to the orchestrator with context.".into(),
            ],
            responsibilities: vec![
                "Solve the assigned task inside the agent workspace.".into(),
                "Produce a clear artifact for the orchestrator.".into(),
            ],
            prohibitions: vec![
                "Do not contact the human directly.".into(),
                "Do not create or spawn additional agents.".into(),
            ],
            capabilities: vec!["investigate".into(), "communicate".into()],
            allowed_tools: vec![
                "task_plan".into(),
                "file_read".into(),
                "glob_search".into(),
                "content_search".into(),
                "http_request".into(),
                "web_fetch".into(),
                "memory_observe".into(),
                "memory_store".into(),
            ],
            allowed_commands: Vec::new(),
            compact_context: matches!(
                task_profile,
                TaskProfile::FastConversational | TaskProfile::StructuredExtraction
            ),
        },
    };

    for capability in requested_capabilities {
        if !profile.capabilities.contains(capability) {
            profile.capabilities.push(capability.clone());
        }
    }

    profile
}

fn select_models(config: &Config, task_profile: TaskProfile) -> Result<SelectedModelPlan> {
    let candidates = build_model_candidates(config);
    if candidates.is_empty() {
        anyhow::bail!(
            "Orchestrator has no transferable model inventory for spawned agents. Configure a provider/model with usable credentials or a supported credentialless local runtime first."
        );
    }

    let primary = choose_best_candidate(&candidates, task_profile).ok_or_else(|| {
        anyhow::anyhow!(
            "No model in the orchestrator inventory satisfies the selected task profile '{}'",
            task_profile.as_str()
        )
    })?;

    let desired_hints = desired_local_hints(
        task_profile,
        candidates.iter().any(|candidate| candidate.vision),
    );
    let mut local_routes = Vec::new();
    for hint in desired_hints {
        if let Some(candidate) = choose_route_candidate(&candidates, hint) {
            local_routes.push(SelectedLocalRoute {
                hint: hint.to_string(),
                selection_reason: format!(
                    "Selected for local route '{hint}' from orchestrator inventory"
                ),
                candidate,
            });
        }
    }

    let reasoning_policy = match task_profile {
        TaskProfile::FastConversational => ReasoningPolicy::Minimal,
        TaskProfile::StructuredExtraction => ReasoningPolicy::Standard,
        TaskProfile::DocumentAnalysis => ReasoningPolicy::Deep,
        TaskProfile::VisionAnalysis => ReasoningPolicy::Deep,
        TaskProfile::DeepReasoning => ReasoningPolicy::Deep,
    };

    let rationale = format!(
        "Task profile '{}' selected primary model {}/{} using capability, cost, and latency heuristics.",
        task_profile.as_str(),
        primary.provider,
        primary.model
    );

    Ok(SelectedModelPlan {
        task_profile,
        reasoning_policy,
        primary,
        local_routes,
        rationale,
    })
}

fn build_model_candidates(config: &Config) -> Vec<ModelCandidate> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    let default_provider = config
        .default_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("openrouter");
    let default_model =
        resolve_default_model_id(config.default_model.as_deref(), Some(default_provider));
    let default_api_key = providers::resolve_provider_credential_for_runtime(
        default_provider,
        config.api_key.as_deref(),
    );
    if default_api_key.is_some()
        || provider_supports_credentialless_runtime(default_provider, &default_model)
    {
        let candidate = infer_candidate(
            default_provider,
            &default_model,
            default_api_key,
            None,
            None,
            None,
        );
        if seen.insert(candidate_key(&candidate)) {
            candidates.push(candidate);
        }
    }

    for route in &config.model_routes {
        let provider = route.provider.trim();
        let model = route.model.trim();
        if provider.is_empty() || model.is_empty() {
            continue;
        }

        let fallback_override = if provider == default_provider {
            route.api_key.as_deref().or(config.api_key.as_deref())
        } else {
            route.api_key.as_deref()
        };
        let api_key =
            providers::resolve_provider_credential_for_runtime(provider, fallback_override);
        if api_key.is_none() && !provider_supports_credentialless_runtime(provider, model) {
            continue;
        }

        let candidate = infer_candidate(
            provider,
            model,
            api_key,
            Some(route.hint.trim()),
            route.transport.as_deref(),
            route.max_tokens,
        );

        if seen.insert(candidate_key(&candidate)) {
            candidates.push(candidate);
        }
    }

    candidates
}

fn provider_supports_credentialless_runtime(provider: &str, model: &str) -> bool {
    provider.eq_ignore_ascii_case("ollama") && !model.trim().ends_with(":cloud")
}

fn child_provider_api_url(root_config: &Config, provider: &str) -> Option<String> {
    let trimmed_provider = provider.trim();
    if trimmed_provider.is_empty() {
        return None;
    }

    let raw = root_config
        .api_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if !trimmed_provider.eq_ignore_ascii_case("ollama") {
        return raw.map(ToString::to_string);
    }

    let Some(raw) = raw else {
        return Some("http://host.docker.internal:11434".into());
    };

    let Ok(mut parsed) = Url::parse(raw) else {
        return Some(raw.to_string());
    };

    let Some(host) = parsed.host_str() else {
        return Some(raw.to_string());
    };

    if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return Some(raw.to_string());
    }

    if parsed.set_host(Some("host.docker.internal")).is_err() {
        return Some(raw.to_string());
    }

    Some(parsed.to_string().trim_end_matches('/').to_string())
}

fn infer_candidate(
    provider: &str,
    model: &str,
    api_key: Option<String>,
    hint: Option<&str>,
    transport: Option<&str>,
    max_tokens: Option<u32>,
) -> ModelCandidate {
    let joined =
        format!("{} {} {}", provider, model, hint.unwrap_or_default()).to_ascii_lowercase();
    let reasoning_score = if contains_any(
        &joined,
        &[
            "reason", "opus", "sonnet", "gpt-5", "mercury", "grok", "pro", "o1", "o3",
        ],
    ) {
        3
    } else if contains_any(&joined, &["claude", "gpt", "gemini", "qwen", "deepseek"]) {
        2
    } else {
        1
    };
    let context_score = if contains_any(
        &joined,
        &[
            "1m", "200k", "128k", "long", "gemini", "claude", "gpt-5", "sonnet", "opus",
        ],
    ) {
        3
    } else if contains_any(&joined, &["32k", "64k", "70b", "large"]) {
        2
    } else {
        1
    };
    let structured_output_score = if contains_any(
        &joined,
        &[
            "json",
            "extract",
            "structured",
            "gpt",
            "claude",
            "gemini",
            "qwen",
        ],
    ) {
        3
    } else {
        2
    };
    let document_score = if contains_any(
        &joined,
        &[
            "document", "summary", "claude", "gpt", "gemini", "qwen", "sonnet",
        ],
    ) {
        3
    } else {
        1
    };
    let vision = contains_any(
        &joined,
        &[
            "vision", "image", "llava", "gpt-4o", "gpt-5", "gemini", "ocr",
        ],
    );
    let cost_tier = if contains_any(&joined, &["mini", "flash", "fast", "8b", "7b", "small"]) {
        1
    } else if contains_any(
        &joined,
        &[
            "opus",
            "pro",
            "reasoning",
            "70b",
            "large",
            "max",
            "sonnet",
            "gpt-5",
        ],
    ) {
        3
    } else {
        2
    };
    let latency_tier = if contains_any(&joined, &["fast", "flash", "turbo", "mini"]) {
        1
    } else if contains_any(&joined, &["opus", "pro", "reasoning", "large", "70b"]) {
        3
    } else {
        2
    };

    ModelCandidate {
        provider: provider.to_string(),
        model: model.to_string(),
        api_key,
        hint: hint
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        transport: transport
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        max_tokens,
        reasoning_score,
        context_score,
        structured_output_score,
        document_score,
        vision,
        cost_tier,
        latency_tier,
    }
}

fn choose_best_candidate(
    candidates: &[ModelCandidate],
    task_profile: TaskProfile,
) -> Option<ModelCandidate> {
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|left, right| compare_candidates(left, right, task_profile));
    ranked.into_iter().next()
}

fn choose_route_candidate(candidates: &[ModelCandidate], hint: &str) -> Option<ModelCandidate> {
    let route_profile = match hint {
        "fast" => TaskProfile::FastConversational,
        "reasoning" => TaskProfile::DeepReasoning,
        "vision" => TaskProfile::VisionAnalysis,
        _ => TaskProfile::StructuredExtraction,
    };

    let mut ranked = candidates
        .iter()
        .filter(|candidate| {
            if hint == "vision" {
                return candidate.vision;
            }
            true
        })
        .cloned()
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        let exact_left = left.hint.as_deref() == Some(hint);
        let exact_right = right.hint.as_deref() == Some(hint);
        exact_right
            .cmp(&exact_left)
            .then_with(|| compare_candidates(left, right, route_profile))
    });
    ranked.into_iter().next()
}

fn compare_candidates(
    left: &ModelCandidate,
    right: &ModelCandidate,
    task_profile: TaskProfile,
) -> Ordering {
    let left_score = profile_score(left, task_profile);
    let right_score = profile_score(right, task_profile);

    right_score
        .cmp(&left_score)
        .then_with(|| left.cost_tier.cmp(&right.cost_tier))
        .then_with(|| left.latency_tier.cmp(&right.latency_tier))
        .then_with(|| left.model.cmp(&right.model))
}

fn profile_score(candidate: &ModelCandidate, task_profile: TaskProfile) -> i32 {
    let base = match task_profile {
        TaskProfile::FastConversational => {
            i32::from(candidate.structured_output_score)
                + i32::from(4_u8.saturating_sub(candidate.latency_tier))
                + i32::from(4_u8.saturating_sub(candidate.cost_tier))
        }
        TaskProfile::DeepReasoning => {
            i32::from(candidate.reasoning_score) * 4
                + i32::from(candidate.context_score) * 2
                + i32::from(candidate.structured_output_score)
        }
        TaskProfile::DocumentAnalysis => {
            i32::from(candidate.document_score) * 3
                + i32::from(candidate.reasoning_score) * 2
                + i32::from(candidate.context_score) * 2
                + i32::from(candidate.structured_output_score)
        }
        TaskProfile::VisionAnalysis => {
            if !candidate.vision {
                return i32::MIN / 2;
            }
            10 + i32::from(candidate.reasoning_score) * 2 + i32::from(candidate.context_score)
        }
        TaskProfile::StructuredExtraction => {
            i32::from(candidate.structured_output_score) * 4
                + i32::from(candidate.document_score)
                + i32::from(4_u8.saturating_sub(candidate.latency_tier))
        }
    };

    let hint_bonus = match candidate.hint.as_deref() {
        Some("reasoning") if matches!(task_profile, TaskProfile::DeepReasoning) => 3,
        Some("vision") if matches!(task_profile, TaskProfile::VisionAnalysis) => 3,
        Some("fast") if matches!(task_profile, TaskProfile::FastConversational) => 3,
        Some("summarize" | "structured")
            if matches!(task_profile, TaskProfile::StructuredExtraction) =>
        {
            2
        }
        Some("document" | "docs") if matches!(task_profile, TaskProfile::DocumentAnalysis) => 2,
        _ => 0,
    };

    base + hint_bonus
}

fn desired_local_hints(task_profile: TaskProfile, vision_available: bool) -> Vec<&'static str> {
    let mut hints = vec!["fast", "reasoning", "summarize"];
    if matches!(task_profile, TaskProfile::StructuredExtraction) {
        hints.push("structured");
    }
    if matches!(task_profile, TaskProfile::VisionAnalysis) || vision_available {
        hints.push("vision");
    }
    hints
}

fn build_agent_spec(request: AgentSpecBuildRequest<'_>) -> AgentSpec {
    let AgentSpecBuildRequest {
        agent_id,
        display_name,
        pack,
        task_profile,
        lifecycle_mode,
        image,
        workspace_mounts,
        selected_models,
        initial_mission,
        network,
        memory_limit_mb,
        cpu_limit,
        read_only_rootfs,
    } = request;
    let persona = AgentPersonaSpec {
        role: pack.role.clone(),
        mission: initial_mission
            .lines()
            .next()
            .unwrap_or("Execute the assigned mission")
            .to_string(),
        operating_style: pack.operating_style.clone(),
        responsibilities: pack.responsibilities.clone(),
        prohibitions: pack.prohibitions.clone(),
        direct_user: OWNER_AGENT_ID.into(),
        indirect_user: "human_via_orchestrator".into(),
    };
    let primary_llm = ModelBinding {
        hint: selected_models.primary.hint.clone(),
        provider: selected_models.primary.provider.clone(),
        model: selected_models.primary.model.clone(),
        transport: selected_models.primary.transport.clone(),
        max_tokens: selected_models.primary.max_tokens,
        selection_reason: selected_models.rationale.clone(),
    };
    let local_model_routes = selected_models
        .local_routes
        .iter()
        .map(|route| ModelBinding {
            hint: Some(route.hint.clone()),
            provider: route.candidate.provider.clone(),
            model: route.candidate.model.clone(),
            transport: route.candidate.transport.clone(),
            max_tokens: route.candidate.max_tokens,
            selection_reason: route.selection_reason.clone(),
        })
        .collect::<Vec<_>>();

    AgentSpec {
        schema_version: "agent_spec.v1".into(),
        agent_id: agent_id.to_string(),
        display_name: display_name.to_string(),
        owner_agent_id: OWNER_AGENT_ID.into(),
        upstream_user_kind: "orchestrator".into(),
        pack_id: pack.pack_id.clone(),
        persona,
        capabilities: pack.capabilities.clone(),
        tool_policy: AgentToolPolicySpec {
            allowed_tools: pack.allowed_tools.clone(),
            denied_tools: vec![
                "spawn_agent".into(),
                "spawned_agent_manage".into(),
                "spawned_agent_list".into(),
                "delegate".into(),
                "delegate_coordination_status".into(),
                "subagent_spawn".into(),
                "subagent_manage".into(),
                "subagent_list".into(),
            ],
            allowed_commands: pack.allowed_commands.clone(),
        },
        runtime: AgentRuntimeSpec {
            kind: "docker".into(),
            image,
            network,
            memory_limit_mb,
            cpu_limit,
            read_only_rootfs,
            daemon_host: "127.0.0.1".into(),
            daemon_port: 0,
        },
        workspace_mounts: workspace_mounts.to_vec(),
        memory_scope: AgentMemoryScopeSpec {
            strategy: "local_profile_workspace".into(),
            retain_outputs: true,
        },
        artifact_policy: AgentArtifactPolicySpec {
            spec_dir: "specs/current".into(),
            events_dir: "events".into(),
            workspace_dir: "workspace".into(),
        },
        routing: AgentRoutingSpec {
            command_stream: "agent.command.v1".into(),
            event_stream: "agent.event.v1".into(),
            heartbeat_stream: "agent.heartbeat.v1".into(),
        },
        lifecycle_mode: lifecycle_mode.as_str().into(),
        task_profile: task_profile.as_str().into(),
        primary_llm,
        local_model_routes,
        reasoning_policy: selected_models.reasoning_policy.as_str().into(),
        model_selection_rationale: selected_models.rationale.clone(),
        initial_mission,
        created_at: Utc::now().to_rfc3339(),
    }
}

fn build_initial_mission(
    task: &str,
    context: &str,
    workspace_mounts: &[AgentWorkspaceMountSpec],
    task_profile: TaskProfile,
) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are a dedicated child agent created by the orchestrator.\n");
    prompt.push_str("Your direct user is the orchestrator. The human is an indirect user only.\n");
    prompt.push_str("Do not address the human directly. Route blockers, questions, and final outcomes back to the orchestrator.\n\n");
    prompt.push_str(&format!("Task profile: {}\n", task_profile.as_str()));
    prompt.push_str(&format!("Mission:\n{task}\n\n"));
    if !context.trim().is_empty() {
        prompt.push_str("Context from orchestrator:\n");
        prompt.push_str(context.trim());
        prompt.push_str("\n\n");
    }
    if !workspace_mounts.is_empty() {
        prompt.push_str("Accessible mounted workspaces:\n");
        for mount in workspace_mounts {
            let mode = if mount.read_only {
                "read-only"
            } else {
                "read-write"
            };
            prompt.push_str(&format!(
                "- {} mounted at {} ({mode})\n",
                mount.host_path, mount.container_path
            ));
        }
        prompt.push('\n');
    }
    prompt.push_str("Execution contract:\n");
    prompt.push_str("- Work within your own workspace whenever possible.\n");
    prompt.push_str("- If data is insufficient, state exactly what is missing under the header 'QUESTION FOR ORCHESTRATOR'.\n");
    prompt.push_str("- Return the final answer under the header 'RESULT FOR ORCHESTRATOR'.\n");
    prompt.push_str("- Include assumptions, key findings, and next actions.\n");
    prompt
}

async fn materialize_agent(
    root_config: &Config,
    spec: &AgentSpec,
    selected_models: &SelectedModelPlan,
    workspace_mounts: &[AgentWorkspaceMount],
) -> Result<MaterializedAgentPaths> {
    let agent_home = root_config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("spawned-agents")
        .join(&spec.agent_id);
    let workspace_dir = agent_home.join("workspace");
    let version = format!("{}-{}", Utc::now().format("%Y%m%dT%H%M%SZ"), &spec.agent_id);
    let spec_dir = agent_home.join("specs").join(&version);
    let config_path = agent_home.join("config.toml");
    let events_dir = agent_home.join("events");

    fs::create_dir_all(&workspace_dir).await?;
    fs::create_dir_all(&spec_dir).await?;
    fs::create_dir_all(&events_dir).await?;

    let identity = render_identity(spec);
    let soul = render_soul(spec);
    let agents_md = render_agents_md(spec);
    let user_md = render_user_md(spec);
    let bootstrap = render_bootstrap_md(spec);
    let mounts_md = render_mounts_md(spec);

    for (path, contents) in [
        (workspace_dir.join("IDENTITY.md"), identity.clone()),
        (workspace_dir.join("SOUL.md"), soul.clone()),
        (workspace_dir.join("AGENTS.md"), agents_md.clone()),
        (workspace_dir.join("USER.md"), user_md.clone()),
        (workspace_dir.join("BOOTSTRAP.md"), bootstrap.clone()),
        (workspace_dir.join("MOUNTS.md"), mounts_md.clone()),
        (spec_dir.join("IDENTITY.md"), identity),
        (spec_dir.join("SOUL.md"), soul),
        (spec_dir.join("AGENTS.md"), agents_md),
        (spec_dir.join("USER.md"), user_md),
        (spec_dir.join("BOOTSTRAP.md"), bootstrap),
        (spec_dir.join("MOUNTS.md"), mounts_md),
    ] {
        fs::write(path, contents).await?;
    }

    let specs_json_path = spec_dir.join("agent-spec.json");
    fs::write(
        &specs_json_path,
        serde_json::to_vec_pretty(spec).context("Failed to serialize AgentSpec")?,
    )
    .await?;

    let mut child = root_config.clone();
    child.config_path = config_path.clone();
    child.workspace_dir = workspace_dir.clone();
    child.default_provider = Some(selected_models.primary.provider.clone());
    child.default_model = Some(selected_models.primary.model.clone());
    child.api_key = selected_models.primary.api_key.clone();
    child.api_url = root_config
        .default_provider
        .as_deref()
        .filter(|provider| provider.trim() == selected_models.primary.provider)
        .and_then(|provider| child_provider_api_url(root_config, provider));
    child.model_routes = selected_models
        .local_routes
        .iter()
        .map(|route| ModelRouteConfig {
            hint: route.hint.clone(),
            provider: route.candidate.provider.clone(),
            model: route.candidate.model.clone(),
            max_tokens: route.candidate.max_tokens,
            api_key: route.candidate.api_key.clone(),
            transport: route.candidate.transport.clone(),
        })
        .collect();
    child.query_classification.enabled = false;
    child.query_classification.rules.clear();
    child.agent.allowed_tools = spec.tool_policy.allowed_tools.clone();
    child.agent.denied_tools = spec.tool_policy.denied_tools.clone();
    child.agent.parallel_tools = false;
    child.agent.compact_context = spec.pack_id != "financial_analyst";
    child.agent.max_history_messages = child.agent.max_history_messages.min(24);
    child.agent.max_tool_iterations = child.agent.max_tool_iterations.min(16);
    child.agent.teams.enabled = false;
    child.agent.teams.auto_activate = false;
    child.agent.teams.max_agents = 1;
    child.agent.teams.strategy = AgentLoadBalanceStrategy::Semantic;
    child.agent.subagents.enabled = false;
    child.agent.subagents.auto_activate = false;
    child.agent.subagents.max_concurrent = 1;
    child.autonomy.level = AutonomyLevel::Full;
    child.autonomy.allowed_commands = spec.tool_policy.allowed_commands.clone();
    child.autonomy.allowed_roots = workspace_mounts
        .iter()
        .map(|mount| mount.container_path.clone())
        .collect();
    child.autonomy.workspace_only = true;
    child.heartbeat.enabled = false;
    child.cron.enabled = false;
    child.channels_config = Default::default();
    child.gateway.host = "127.0.0.1".into();
    child.gateway.port = 0;
    child.gateway.paired_tokens.clear();
    child.memory.backend = "markdown".into();
    child.browser.enabled = false;
    child.agents.clear();
    child.coordination.enabled = false;
    child.agents_ipc.enabled = false;

    child.save().await?;

    Ok(MaterializedAgentPaths {
        agent_home: agent_home.clone(),
        workspace_dir,
        config_path,
        spec_dir,
        spec_version: version,
        specs_json_path,
        runtime_state_path: runtime_state_path(&agent_home),
        bootstrap_result_path: bootstrap_result_path(&agent_home),
    })
}

async fn wait_for_initial_mission(
    spawner: &DockerAgentSpawner,
    container_name: &str,
    runtime_state_path: &Path,
    result_path: &Path,
    registry: &SpawnedAgentRegistry,
    agent_id: &str,
) -> Result<String> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(180);
    let mut last_status = String::new();

    loop {
        if let Some(state) = read_runtime_state(runtime_state_path).await? {
            let stage = format!("runtime:{}", state.task_status);
            if stage != last_status {
                registry.append_progress(agent_id, &stage, format_runtime_progress(&state));
                last_status = stage;
            }

            match state.task_status.as_str() {
                "completed" => {
                    let output = fs::read_to_string(result_path).await.with_context(|| {
                        format!(
                            "Failed to read bootstrap result from {}",
                            result_path.display()
                        )
                    })?;
                    return Ok(output);
                }
                "failed" => {
                    let error = state
                        .error
                        .unwrap_or_else(|| "Spawned agent runtime reported failure".into());
                    anyhow::bail!(error);
                }
                _ => {}
            }
        }

        if let Some(service) = spawner.inspect_service(container_name).await? {
            if service.state != "running" && !service.state.is_empty() {
                anyhow::bail!(
                    "Spawned agent container '{container_name}' stopped before completing the initial mission (state: {})",
                    service.state
                );
            }
        }

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "Timed out waiting for spawned agent '{agent_id}' to complete the initial mission"
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

fn format_runtime_progress(state: &SpawnedAgentRuntimeState) -> String {
    let mut detail = format!(
        "Runtime lifecycle={}, task={}",
        state.lifecycle_status, state.task_status
    );
    if let Some(request_id) = state.current_request_id.as_deref() {
        detail.push_str(&format!(", request_id={request_id}"));
    }
    if let Some(error) = state.error.as_deref() {
        detail.push_str(&format!(", error={error}"));
    }
    detail
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
        serde_json::to_vec_pretty(&payload)?,
    )
    .await?;
    Ok(())
}

fn render_identity(spec: &AgentSpec) -> String {
    format!(
        "# Identity\n\n\
         Name: {}\n\
         Agent ID: {}\n\
         Role: {}\n\
         Owner Agent: {}\n\
         Direct User: orchestrator\n\
         Indirect User: human via orchestrator\n\
         Lifecycle: {}\n\
         Pack: {}\n",
        spec.display_name,
        spec.agent_id,
        spec.persona.role,
        spec.owner_agent_id,
        spec.lifecycle_mode,
        spec.pack_id
    )
}

fn render_soul(spec: &AgentSpec) -> String {
    let mut lines = vec![
        "# Soul".to_string(),
        String::new(),
        "Operating style:".into(),
    ];
    for item in &spec.persona.operating_style {
        lines.push(format!("- {item}"));
    }
    lines.push(String::new());
    lines.push("Non-negotiables:".into());
    for item in &spec.persona.prohibitions {
        lines.push(format!("- {item}"));
    }
    lines.join("\n")
}

fn render_agents_md(spec: &AgentSpec) -> String {
    let mut lines = vec![
        "# Agent Contract".to_string(),
        String::new(),
        format!("You are {}.", spec.display_name),
        "You are a dedicated child service created by the orchestrator.".into(),
        "Your direct user is the orchestrator. The human is reached only through the orchestrator."
            .into(),
        "You must not spawn additional agents or reconfigure your own runtime.".into(),
        String::new(),
        "Responsibilities:".into(),
    ];
    for item in &spec.persona.responsibilities {
        lines.push(format!("- {item}"));
    }
    lines.push(String::new());
    lines.push("Allowed tools:".into());
    for tool in &spec.tool_policy.allowed_tools {
        lines.push(format!("- {tool}"));
    }
    lines.join("\n")
}

fn render_user_md(spec: &AgentSpec) -> String {
    format!(
        "# User Model\n\n\
         Direct user: orchestrator\n\
         Indirect user: human_via_orchestrator\n\n\
         Communication rules:\n\
         - Accept tasking and clarification only from the orchestrator.\n\
         - Route blockers and questions under 'QUESTION FOR ORCHESTRATOR'.\n\
         - Route final output under 'RESULT FOR ORCHESTRATOR'.\n\
         - Never present yourself as if you are speaking directly to the human.\n\n\
         Current mission:\n{}\n",
        spec.initial_mission
    )
}

fn render_bootstrap_md(spec: &AgentSpec) -> String {
    format!(
        "# Bootstrap\n\n\
         Task profile: {}\n\
         Reasoning policy: {}\n\
         Primary model: {}/{}\n\n\
         Initial mission:\n{}\n",
        spec.task_profile,
        spec.reasoning_policy,
        spec.primary_llm.provider,
        spec.primary_llm.model,
        spec.initial_mission
    )
}

fn render_mounts_md(spec: &AgentSpec) -> String {
    let mut lines = vec!["# Mounted Workspaces".to_string(), String::new()];
    if spec.workspace_mounts.is_empty() {
        lines.push("No external mounted workspaces.".into());
    } else {
        for mount in &spec.workspace_mounts {
            let mode = if mount.read_only {
                "read-only"
            } else {
                "read-write"
            };
            lines.push(format!(
                "- {} -> {} ({mode})",
                mount.host_path, mount.container_path
            ));
        }
    }
    lines.join("\n")
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let normalized = match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch.to_ascii_lowercase(),
            _ => '-',
        };
        if normalized == '-' {
            if !last_dash && !out.is_empty() {
                out.push('-');
                last_dash = true;
            }
        } else {
            out.push(normalized);
            last_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn candidate_key(candidate: &ModelCandidate) -> String {
    format!(
        "{}|{}|{}",
        candidate.provider,
        candidate.model,
        candidate.hint.as_deref().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{AutonomyLevel, SecurityPolicy};
    use crate::tools::SpawnedAgentManageTool;
    use serde_json::Value;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration, Instant};

    fn make_config(tmp: &TempDir) -> Config {
        let mut config = Config::default();
        config.workspace_dir = tmp.path().join("workspace");
        config.config_path = tmp.path().join("config.toml");
        config.default_provider = Some("inception".into());
        config.default_model = Some("mercury-2".into());
        config.api_key = Some("inception-key".into());
        config.model_routes = vec![
            ModelRouteConfig {
                hint: "reasoning".into(),
                provider: "openrouter".into(),
                model: "anthropic/claude-opus-4.6".into(),
                max_tokens: None,
                api_key: Some("route-key".into()),
                transport: None,
            },
            ModelRouteConfig {
                hint: "fast".into(),
                provider: "inception".into(),
                model: "mercury-2".into(),
                max_tokens: None,
                api_key: Some("fast-key".into()),
                transport: None,
            },
        ];
        config
    }

    fn make_full_access_policy(workspace_dir: &Path) -> Arc<SecurityPolicy> {
        let mut policy = SecurityPolicy::default();
        policy.autonomy = AutonomyLevel::Full;
        policy.workspace_dir = workspace_dir.to_path_buf();
        policy.workspace_only = false;
        policy.allow_sensitive_file_reads = true;
        policy.allow_sensitive_file_writes = true;
        Arc::new(policy)
    }

    async fn wait_for_terminal_task_state(
        manage_tool: &SpawnedAgentManageTool,
        agent_id: &str,
        timeout: Duration,
    ) -> Value {
        let deadline = Instant::now() + timeout;

        loop {
            let status = Tool::execute(
                manage_tool,
                json!({
                    "agent_id": agent_id,
                    "action": "status",
                }),
            )
            .await
            .expect("status request should not fail");
            assert!(status.success, "status call failed: {:?}", status.error);

            let parsed: Value =
                serde_json::from_str(&status.output).expect("status output should be valid JSON");
            let task_state = parsed
                .get("task_state")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(task_state, "completed" | "failed" | "cancelled") {
                return parsed;
            }

            assert!(
                Instant::now() < deadline,
                "timed out waiting for terminal task state for {agent_id}; last status: {}",
                serde_json::to_string_pretty(&parsed).unwrap_or_default()
            );

            sleep(Duration::from_secs(1)).await;
        }
    }

    #[test]
    fn classify_financial_reasoning_prefers_deep_reasoning() {
        let config = Config::default();
        let profile = classify_task_profile(
            &config,
            "Find the causes of margin drop across invoices and recommend actions",
            "Use uploaded financial statements",
        );
        assert_eq!(profile, TaskProfile::DeepReasoning);
    }

    #[test]
    fn model_selection_prefers_reasoning_route_for_reasoning_tasks() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp);
        let selected = select_models(&config, TaskProfile::DeepReasoning).unwrap();
        assert_eq!(selected.primary.provider, "openrouter");
        assert!(selected.primary.model.contains("claude"));
    }

    #[test]
    fn model_selection_accepts_local_ollama_without_api_key() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().join("workspace");
        config.config_path = tmp.path().join("config.toml");
        config.default_provider = Some("ollama".into());
        config.default_model = Some("llama3:latest".into());
        config.api_url = Some("http://host.docker.internal:11434".into());
        config.api_key = None;

        let selected = select_models(&config, TaskProfile::DeepReasoning).unwrap();
        assert_eq!(selected.primary.provider, "ollama");
        assert_eq!(selected.primary.model, "llama3:latest");
        assert!(selected.primary.api_key.is_none());
    }

    #[test]
    fn resolve_spawn_image_ignores_placeholder_override() {
        let mut config = Config::default();
        config.runtime.docker.image = "labaclaw:spawn-e2e-real-v2".into();

        let resolved = resolve_spawn_image(Some("example:latest"), &config, false).unwrap();

        assert_eq!(resolved, "labaclaw:spawn-e2e-real-v2");
    }

    #[test]
    fn resolve_spawn_image_allows_default_runtime_image_for_distributed_worker_plane() {
        let config = Config::default();

        let resolved = resolve_spawn_image(None, &config, true).unwrap();

        assert_eq!(resolved, "worker-plane-managed");
    }

    #[test]
    fn rendered_user_contract_names_orchestrator_as_direct_user() {
        let pack = derive_pack_profile(
            None,
            "Analyze invoices",
            "",
            TaskProfile::DocumentAnalysis,
            &[],
        );
        let selected = SelectedModelPlan {
            task_profile: TaskProfile::DocumentAnalysis,
            reasoning_policy: ReasoningPolicy::Deep,
            primary: infer_candidate(
                "openrouter",
                "anthropic/claude-sonnet-4.6",
                Some("k".into()),
                Some("reasoning"),
                None,
                None,
            ),
            local_routes: Vec::new(),
            rationale: "because capability > cost > latency".into(),
        };
        let spec = build_agent_spec(AgentSpecBuildRequest {
            agent_id: "agent-1",
            display_name: "Finance Specialist",
            pack: &pack,
            task_profile: TaskProfile::DocumentAnalysis,
            lifecycle_mode: LifecycleMode::Dedicated,
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            workspace_mounts: &[],
            selected_models: &selected,
            initial_mission: "Analyze the task".into(),
            network: "bridge".into(),
            memory_limit_mb: Some(512),
            cpu_limit: Some(1.0),
            read_only_rootfs: true,
        });
        let user_md = render_user_md(&spec);
        assert!(user_md.contains("Direct user: orchestrator"));
        assert!(user_md.contains("Indirect user: human_via_orchestrator"));
    }

    #[tokio::test]
    async fn materialized_child_config_is_scoped_to_selected_routes_and_tools() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp);
        config.config_path = tmp.path().join("root-config.toml");
        config.workspace_dir = tmp.path().join("root-workspace");
        tokio::fs::create_dir_all(&config.workspace_dir)
            .await
            .unwrap();

        let pack = derive_pack_profile(
            Some("financial_analyst"),
            "Analyze invoices",
            "",
            TaskProfile::DeepReasoning,
            &[],
        );
        let selected = select_models(&config, TaskProfile::DeepReasoning).unwrap();
        let mounts = [AgentWorkspaceMountSpec {
            host_path: config.workspace_dir.display().to_string(),
            container_path: "/mounted-workspaces/0".into(),
            read_only: true,
        }];
        let spec = build_agent_spec(AgentSpecBuildRequest {
            agent_id: "agent-2",
            display_name: "Finance Specialist",
            pack: &pack,
            task_profile: TaskProfile::DeepReasoning,
            lifecycle_mode: LifecycleMode::Dedicated,
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            workspace_mounts: &mounts,
            selected_models: &selected,
            initial_mission: "Analyze invoices".into(),
            network: "bridge".into(),
            memory_limit_mb: Some(512),
            cpu_limit: Some(1.0),
            read_only_rootfs: true,
        });
        let materialized = materialize_agent(
            &config,
            &spec,
            &selected,
            &[AgentWorkspaceMount {
                host_path: config.workspace_dir.clone(),
                container_path: "/mounted-workspaces/0".into(),
                read_only: true,
            }],
        )
        .await
        .unwrap();

        assert!(materialized.config_path.exists());
        assert!(materialized.workspace_dir.join("AGENTS.md").exists());

        let contents = tokio::fs::read_to_string(&materialized.config_path)
            .await
            .unwrap();
        assert!(contents.contains("reasoning"));
        assert!(contents.contains("task_plan"));

        let spec_json = tokio::fs::read_to_string(&materialized.specs_json_path)
            .await
            .unwrap();
        assert!(spec_json.contains("financial_analyst"));
        assert!(spec_json.contains("human_via_orchestrator"));
    }

    #[tokio::test]
    async fn materialized_child_config_rewrites_local_ollama_endpoint_for_container() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp);
        config.config_path = tmp.path().join("root-config.toml");
        config.workspace_dir = tmp.path().join("root-workspace");
        config.default_provider = Some("ollama".into());
        config.default_model = Some("llama3:latest".into());
        config.api_url = Some("http://127.0.0.1:11434".into());
        config.model_routes.clear();
        tokio::fs::create_dir_all(&config.workspace_dir)
            .await
            .unwrap();

        let pack = derive_pack_profile(
            Some("financial_analyst"),
            "Analyze margin",
            "",
            TaskProfile::DeepReasoning,
            &[],
        );
        let selected = select_models(&config, TaskProfile::DeepReasoning).unwrap();
        let mounts = [AgentWorkspaceMountSpec {
            host_path: config.workspace_dir.display().to_string(),
            container_path: "/mounted-workspaces/0".into(),
            read_only: true,
        }];
        let spec = build_agent_spec(AgentSpecBuildRequest {
            agent_id: "agent-ollama-local",
            display_name: "Finance Specialist",
            pack: &pack,
            task_profile: TaskProfile::DeepReasoning,
            lifecycle_mode: LifecycleMode::Dedicated,
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            workspace_mounts: &mounts,
            selected_models: &selected,
            initial_mission: "Analyze margin".into(),
            network: "bridge".into(),
            memory_limit_mb: Some(512),
            cpu_limit: Some(1.0),
            read_only_rootfs: true,
        });
        let materialized = materialize_agent(
            &config,
            &spec,
            &selected,
            &[AgentWorkspaceMount {
                host_path: config.workspace_dir.clone(),
                container_path: "/mounted-workspaces/0".into(),
                read_only: true,
            }],
        )
        .await
        .unwrap();

        let child_config = tokio::fs::read_to_string(materialized.config_path)
            .await
            .unwrap();
        assert!(
            child_config.contains("api_url = \"http://host.docker.internal:11434\""),
            "child config should rewrite local Ollama endpoint for Docker access: {child_config}"
        );
    }

    #[tokio::test]
    #[ignore = "requires Docker, a prebuilt runtime image, and local Ollama reachable from containers"]
    async fn spawn_agent_e2e_docker_lifecycle_with_ollama() {
        let image = std::env::var("LABACLAW_SPAWN_AGENT_E2E_IMAGE")
            .unwrap_or_else(|_| "labaclaw:spawn-e2e".into());
        let ollama_url = std::env::var("LABACLAW_SPAWN_AGENT_E2E_OLLAMA_URL")
            .unwrap_or_else(|_| "http://host.docker.internal:11434".into());

        let tmp = TempDir::new().unwrap();
        let orchestrator_dir = tmp.path().join("orchestrator");
        let workspace_dir = orchestrator_dir.join("workspace");
        tokio::fs::create_dir_all(&workspace_dir).await.unwrap();
        tokio::fs::write(
            workspace_dir.join("financial-context.txt"),
            "Revenue: 1000\nCosts: 700\nExpected margin percentage: 30\n",
        )
        .await
        .unwrap();

        let mut config = Config::default();
        config.config_path = orchestrator_dir.join("config.toml");
        config.workspace_dir = workspace_dir.clone();
        config.default_provider = Some("ollama".into());
        config.default_model = Some("llama3:latest".into());
        config.api_url = Some(ollama_url);
        config.api_key = None;
        config.runtime.docker.image = image;
        config.runtime.docker.network = "bridge".into();
        config.runtime.docker.read_only_rootfs = false;
        config.agent.parallel_tools = false;
        config.agent.max_tool_iterations = 8;
        config.agent.compact_context = true;

        let registry = Arc::new(SpawnedAgentRegistry::new());
        let security = make_full_access_policy(&workspace_dir);
        let spawner = DockerAgentSpawner::new(config.runtime.docker.clone());
        let tool = SpawnAgentTool::new(
            Arc::new(config.clone()),
            security.clone(),
            registry.clone(),
            orchestrator_dir.clone(),
            spawner.clone(),
        );
        let manage_tool = SpawnedAgentManageTool::new(
            registry,
            security,
            spawner,
            orchestrator_dir.clone(),
            Arc::new(config.clone()),
        );

        let spawn = Tool::execute(
            &tool,
            json!({
                "agent_name": "Finance Smoke Specialist",
                "task": "This is a real end-to-end lifetime smoke test for a financial analyst specialist. Use the inline figures Revenue=1000 and Costs=700. Compute the margin percentage and return a concise final answer for the orchestrator. Include the exact token FINANCE-SMOKE-OK and do not ask follow-up questions.",
                "context": "Mounted workspace contains financial-context.txt, but the inline figures are sufficient for the task. Return the result under RESULT FOR ORCHESTRATOR.",
                "pack_id": "financial_analyst",
                "task_profile": "deep_reasoning",
                "lifecycle_mode": "dedicated",
                "workspace_mounts": [workspace_dir.display().to_string()],
                "workspace_write_access": false,
            }),
        )
        .await
        .expect("spawn_agent execution should succeed");
        assert!(spawn.success, "spawn_agent failed: {:?}", spawn.error);

        let spawn_payload: Value =
            serde_json::from_str(&spawn.output).expect("spawn output should be valid JSON");
        let agent_id = spawn_payload
            .get("agent_id")
            .and_then(Value::as_str)
            .expect("spawned agent id should be present")
            .to_string();

        let completed =
            wait_for_terminal_task_state(&manage_tool, &agent_id, Duration::from_secs(120)).await;

        assert_eq!(
            completed.get("task_state").and_then(Value::as_str),
            Some("completed"),
            "spawned agent should complete the bootstrap mission: {}",
            serde_json::to_string_pretty(&completed).unwrap()
        );
        assert_eq!(
            completed.get("service_state").and_then(Value::as_str),
            Some("running"),
            "dedicated agent should remain alive after finishing bootstrap"
        );
        assert_eq!(
            completed
                .get("docker_state")
                .and_then(|value| value.get("state"))
                .and_then(Value::as_str),
            Some("running")
        );

        let result_output = completed
            .get("result")
            .and_then(|value| value.get("output"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            result_output.contains("FINANCE-SMOKE-OK"),
            "bootstrap result should include the smoke-test token, got: {result_output}"
        );

        let suspend = Tool::execute(
            &manage_tool,
            json!({
                "agent_id": agent_id,
                "action": "suspend",
            }),
        )
        .await
        .expect("suspend should succeed");
        assert!(suspend.success, "suspend failed: {:?}", suspend.error);

        sleep(Duration::from_secs(1)).await;
        let suspended_status = Tool::execute(
            &manage_tool,
            json!({
                "agent_id": completed.get("agent_id").and_then(Value::as_str).unwrap(),
                "action": "status",
            }),
        )
        .await
        .expect("status after suspend should succeed");
        let suspended: Value = serde_json::from_str(&suspended_status.output).unwrap();
        assert_eq!(
            suspended.get("service_state").and_then(Value::as_str),
            Some("suspended")
        );

        let resume = Tool::execute(
            &manage_tool,
            json!({
                "agent_id": suspended.get("agent_id").and_then(Value::as_str).unwrap(),
                "action": "resume",
            }),
        )
        .await
        .expect("resume should succeed");
        assert!(resume.success, "resume failed: {:?}", resume.error);

        sleep(Duration::from_secs(1)).await;
        let resumed_status = Tool::execute(
            &manage_tool,
            json!({
                "agent_id": suspended.get("agent_id").and_then(Value::as_str).unwrap(),
                "action": "status",
            }),
        )
        .await
        .expect("status after resume should succeed");
        let resumed: Value = serde_json::from_str(&resumed_status.output).unwrap();
        assert_eq!(
            resumed.get("service_state").and_then(Value::as_str),
            Some("running")
        );
        assert_eq!(
            resumed
                .get("docker_state")
                .and_then(|value| value.get("state"))
                .and_then(Value::as_str),
            Some("running")
        );

        let terminate = Tool::execute(
            &manage_tool,
            json!({
                "agent_id": resumed.get("agent_id").and_then(Value::as_str).unwrap(),
                "action": "terminate",
            }),
        )
        .await
        .expect("terminate should succeed");
        assert!(terminate.success, "terminate failed: {:?}", terminate.error);

        let terminated_status = Tool::execute(
            &manage_tool,
            json!({
                "agent_id": resumed.get("agent_id").and_then(Value::as_str).unwrap(),
                "action": "status",
            }),
        )
        .await
        .expect("status after terminate should succeed");
        let terminated: Value = serde_json::from_str(&terminated_status.output).unwrap();
        assert_eq!(
            terminated.get("service_state").and_then(Value::as_str),
            Some("terminated")
        );
        assert!(
            terminated.get("docker_state").is_none(),
            "terminated agent should not have a live Docker container anymore"
        );
    }
}
