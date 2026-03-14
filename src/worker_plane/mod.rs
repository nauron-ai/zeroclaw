pub mod projection;

use crate::config::WorkerPlaneConfig;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
#[cfg(feature = "worker-plane-distributed")]
use {
    object_store::aws::AmazonS3Builder,
    object_store::path::Path as ObjectStorePath,
    object_store::ObjectStore,
    rdkafka::config::ClientConfig,
    rdkafka::message::{Header, OwnedHeaders},
    rdkafka::producer::{FutureProducer, FutureRecord},
    std::sync::Arc,
    tokio::time::{timeout, Duration},
};

const CONTROL_PLANE_DIR: &str = "control-plane";
const DISTRIBUTED_SPAWN_PLAN_FILE: &str = "spawn-plan.json";
pub const MESSAGE_TYPE_HEADER: &str = "labaclaw-message-type";
pub const COMMAND_TYPE_SPAWN_AGENT_REQUESTED: &str = "SpawnAgentRequested";
pub const COMMAND_TYPE_SUSPEND_AGENT_REQUESTED: &str = "SuspendAgentRequested";
pub const COMMAND_TYPE_RESUME_AGENT_REQUESTED: &str = "ResumeAgentRequested";
pub const COMMAND_TYPE_TERMINATE_AGENT_REQUESTED: &str = "TerminateAgentRequested";
pub const COMMAND_TYPE_TASK_ASSIGNED: &str = "TaskAssigned";
#[cfg(feature = "worker-plane-distributed")]
const ARTIFACT_IO_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(feature = "worker-plane-distributed")]
const MESSAGE_PUBLISH_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerPlaneArtifactRefs {
    pub spec_ref: String,
    pub bootstrap_ref: String,
    pub result_ref: String,
    pub artifacts_prefix_ref: String,
    pub questions_prefix_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerPlaneTopics {
    pub command_topic: String,
    pub event_topic: String,
    pub heartbeat_topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnAgentRequestedCommand {
    pub event_id: String,
    pub agent_id: String,
    pub owner_agent_id: String,
    pub spec_ref: String,
    pub bootstrap_ref: String,
    pub lifecycle_mode: String,
    pub task_profile: String,
    pub requested_at: String,
    pub delivery_backend: Option<String>,
    pub worker_namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DistributedSpawnPlan {
    pub agent_id: String,
    pub delivery_backend: String,
    pub topics: WorkerPlaneTopics,
    pub artifact_refs: WorkerPlaneArtifactRefs,
    pub spawn_command: SpawnAgentRequestedCommand,
    pub projection_consumer_group: String,
    pub worker_namespace: String,
}

pub const DISTRIBUTED_WORKER_PLANE_FEATURE: &str = "worker-plane-distributed";

pub const DISTRIBUTED_SUPPORT_ERROR: &str = "worker-plane distributed transport is not compiled in; rebuild with feature 'worker-plane-distributed'";

pub fn distributed_support_compiled() -> bool {
    cfg!(feature = "worker-plane-distributed")
}

pub fn worker_plane_enabled(config: &WorkerPlaneConfig) -> bool {
    config.enabled
        && config.mode.trim().eq_ignore_ascii_case("redpanda_k8s")
        && distributed_support_compiled()
}

pub fn local_file_ref(path: &Path) -> String {
    let mut normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.len() >= 2
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[0].is_ascii_alphabetic()
        && !normalized.starts_with('/')
    {
        normalized.insert(0, '/');
    }
    format!("file://{normalized}")
}

pub fn build_artifact_refs(
    config: &WorkerPlaneConfig,
    agent_id: &str,
    spec_version: &str,
    request_id: &str,
) -> WorkerPlaneArtifactRefs {
    let prefix = normalize_prefix(&config.artifacts.prefix);
    let bucket = config.artifacts.bucket.trim();
    let specs_key = join_object_key(
        &prefix,
        &format!("specs/{agent_id}/{spec_version}/agent-spec.json"),
    );
    let bootstrap_key = join_object_key(
        &prefix,
        &format!("bootstrap/{agent_id}/{request_id}/request.json"),
    );
    let result_key = join_object_key(
        &prefix,
        &format!("results/{agent_id}/{request_id}/result.md"),
    );
    let artifacts_prefix_key =
        join_object_key(&prefix, &format!("artifacts/{agent_id}/{request_id}/"));
    let questions_prefix_key =
        join_object_key(&prefix, &format!("questions/{agent_id}/{request_id}/"));

    WorkerPlaneArtifactRefs {
        spec_ref: s3_uri(bucket, &specs_key),
        bootstrap_ref: s3_uri(bucket, &bootstrap_key),
        result_ref: s3_uri(bucket, &result_key),
        artifacts_prefix_ref: s3_uri(bucket, &artifacts_prefix_key),
        questions_prefix_ref: s3_uri(bucket, &questions_prefix_key),
    }
}

pub fn build_topics(config: &WorkerPlaneConfig) -> WorkerPlaneTopics {
    WorkerPlaneTopics {
        command_topic: config.redpanda.command_topic.clone(),
        event_topic: config.redpanda.event_topic.clone(),
        heartbeat_topic: config.redpanda.heartbeat_topic.clone(),
    }
}

pub fn build_spawn_command(
    config: &WorkerPlaneConfig,
    agent_id: &str,
    owner_agent_id: &str,
    lifecycle_mode: &str,
    task_profile: &str,
    artifact_refs: &WorkerPlaneArtifactRefs,
) -> SpawnAgentRequestedCommand {
    SpawnAgentRequestedCommand {
        event_id: uuid::Uuid::new_v4().to_string(),
        agent_id: agent_id.to_string(),
        owner_agent_id: owner_agent_id.to_string(),
        spec_ref: artifact_refs.spec_ref.clone(),
        bootstrap_ref: artifact_refs.bootstrap_ref.clone(),
        lifecycle_mode: lifecycle_mode.to_string(),
        task_profile: task_profile.to_string(),
        requested_at: Utc::now().to_rfc3339(),
        delivery_backend: Some(config.mode.clone()),
        worker_namespace: Some(config.kubernetes.namespace.clone()),
    }
}

pub fn build_local_artifact_refs(
    spec_path: &Path,
    bootstrap_path: &Path,
    result_path: &Path,
    artifacts_dir: &Path,
    questions_dir: &Path,
) -> WorkerPlaneArtifactRefs {
    WorkerPlaneArtifactRefs {
        spec_ref: local_file_ref(spec_path),
        bootstrap_ref: local_file_ref(bootstrap_path),
        result_ref: local_file_ref(result_path),
        artifacts_prefix_ref: local_file_ref(artifacts_dir),
        questions_prefix_ref: local_file_ref(questions_dir),
    }
}

pub fn build_spawn_plan_from_refs(
    config: &WorkerPlaneConfig,
    agent_id: &str,
    owner_agent_id: &str,
    lifecycle_mode: &str,
    task_profile: &str,
    artifact_refs: WorkerPlaneArtifactRefs,
) -> DistributedSpawnPlan {
    DistributedSpawnPlan {
        agent_id: agent_id.to_string(),
        delivery_backend: config.mode.clone(),
        topics: build_topics(config),
        spawn_command: build_spawn_command(
            config,
            agent_id,
            owner_agent_id,
            lifecycle_mode,
            task_profile,
            &artifact_refs,
        ),
        artifact_refs,
        projection_consumer_group: config.redpanda.projection_consumer_group.clone(),
        worker_namespace: config.kubernetes.namespace.clone(),
    }
}

pub fn build_distributed_spawn_plan(
    config: &WorkerPlaneConfig,
    agent_id: &str,
    owner_agent_id: &str,
    lifecycle_mode: &str,
    task_profile: &str,
    spec_version: &str,
    request_id: &str,
) -> DistributedSpawnPlan {
    let artifact_refs = build_artifact_refs(config, agent_id, spec_version, request_id);
    build_spawn_plan_from_refs(
        config,
        agent_id,
        owner_agent_id,
        lifecycle_mode,
        task_profile,
        artifact_refs,
    )
}

pub async fn write_distributed_spawn_plan(
    agent_home: &Path,
    plan: &DistributedSpawnPlan,
) -> Result<PathBuf> {
    let dir = control_plane_dir(agent_home);
    fs::create_dir_all(&dir).await?;
    let plan_path = distributed_spawn_plan_path(agent_home);
    fs::write(
        &plan_path,
        serde_json::to_vec_pretty(plan).context("Failed to serialize distributed spawn plan")?,
    )
    .await
    .with_context(|| format!("Failed to write {}", plan_path.display()))?;
    Ok(plan_path)
}

pub fn control_plane_dir(agent_home: &Path) -> PathBuf {
    agent_home.join(CONTROL_PLANE_DIR)
}

pub fn distributed_spawn_plan_path(agent_home: &Path) -> PathBuf {
    control_plane_dir(agent_home).join(DISTRIBUTED_SPAWN_PLAN_FILE)
}

pub fn spawn_request_payload(plan: &DistributedSpawnPlan) -> serde_json::Value {
    serde_json::to_value(&plan.spawn_command).expect("serialize spawn_command")
}

pub async fn upload_bytes_to_artifact_ref(
    config: &WorkerPlaneConfig,
    artifact_ref: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    #[cfg(not(feature = "worker-plane-distributed"))]
    {
        let _ = (config, artifact_ref, bytes);
        anyhow::bail!(DISTRIBUTED_SUPPORT_ERROR);
    }

    #[cfg(feature = "worker-plane-distributed")]
    {
        let (bucket, key) = parse_s3_uri(artifact_ref)?;
        ensure_bucket_matches_config(config, &bucket)?;
        let store = build_artifact_store(config)?;
        timeout(
            ARTIFACT_IO_TIMEOUT,
            store.put(&ObjectStorePath::from(key), bytes.into()),
        )
        .await
        .with_context(|| format!("Timed out uploading artifact to {artifact_ref}"))?
        .with_context(|| format!("Failed to upload artifact to {artifact_ref}"))?;
        Ok(())
    }
}

pub async fn download_bytes_from_artifact_ref(
    config: &WorkerPlaneConfig,
    artifact_ref: &str,
) -> Result<Vec<u8>> {
    #[cfg(not(feature = "worker-plane-distributed"))]
    {
        let _ = (config, artifact_ref);
        anyhow::bail!(DISTRIBUTED_SUPPORT_ERROR);
    }

    #[cfg(feature = "worker-plane-distributed")]
    {
        let (bucket, key) = parse_s3_uri(artifact_ref)?;
        ensure_bucket_matches_config(config, &bucket)?;
        let store = build_artifact_store(config)?;
        let bytes = timeout(ARTIFACT_IO_TIMEOUT, store.get(&ObjectStorePath::from(key)))
            .await
            .with_context(|| format!("Timed out fetching artifact from {artifact_ref}"))?
            .with_context(|| format!("Failed to fetch artifact from {artifact_ref}"))?
            .bytes();
        let bytes = timeout(ARTIFACT_IO_TIMEOUT, bytes)
            .await
            .with_context(|| format!("Timed out reading artifact bytes from {artifact_ref}"))?
            .with_context(|| format!("Failed to read artifact bytes from {artifact_ref}"))?;
        Ok(bytes.to_vec())
    }
}

pub async fn publish_json_message(
    config: &WorkerPlaneConfig,
    topic: &str,
    message_type: &str,
    agent_id: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    #[cfg(not(feature = "worker-plane-distributed"))]
    {
        let _ = (config, topic, message_type, agent_id, payload);
        anyhow::bail!(DISTRIBUTED_SUPPORT_ERROR);
    }

    #[cfg(feature = "worker-plane-distributed")]
    {
        let producer = build_producer(config)?;
        let serialized =
            serde_json::to_string(payload).context("Failed to serialize worker-plane payload")?;
        let headers = OwnedHeaders::new().insert(Header {
            key: MESSAGE_TYPE_HEADER,
            value: Some(message_type),
        });
        timeout(
            MESSAGE_PUBLISH_TIMEOUT,
            producer.send(
                FutureRecord::to(topic)
                    .payload(&serialized)
                    .key(agent_id)
                    .headers(headers),
                Duration::from_secs(10),
            ),
        )
        .await
        .with_context(|| {
            format!("Timed out publishing {message_type} for agent {agent_id} to topic {topic}")
        })?
        .map_err(|(error, _message)| error)
        .with_context(|| {
            format!("Failed to publish {message_type} for agent {agent_id} to topic {topic}")
        })?;
        Ok(())
    }
}

#[cfg(feature = "worker-plane-distributed")]
fn build_producer(config: &WorkerPlaneConfig) -> Result<FutureProducer> {
    let brokers = config.redpanda.brokers.join(",");
    if brokers.trim().is_empty() {
        anyhow::bail!("worker_plane.redpanda.brokers is empty");
    }
    ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("message.timeout.ms", "10000")
        .create()
        .context("Failed to build Redpanda producer for worker-plane")
}

#[cfg(feature = "worker-plane-distributed")]
fn build_artifact_store(config: &WorkerPlaneConfig) -> Result<Arc<dyn ObjectStore>> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&config.artifacts.bucket)
        .with_region(&config.artifacts.region)
        .with_virtual_hosted_style_request(!config.artifacts.force_path_style);

    if let Some(endpoint) = config.artifacts.endpoint.as_deref() {
        builder = builder
            .with_endpoint(endpoint)
            .with_allow_http(endpoint.trim_start().starts_with("http://"));
    }
    if let Some(access_key) = config.artifacts.access_key.as_deref() {
        builder = builder.with_access_key_id(access_key);
    }
    if let Some(secret_key) = config.artifacts.secret_key.as_deref() {
        builder = builder.with_secret_access_key(secret_key);
    }
    let store = builder
        .build()
        .context("Failed to build RustFS/S3 artifact store client")?;
    Ok(Arc::new(store))
}

pub fn parse_s3_uri(uri: &str) -> Result<(String, String)> {
    let trimmed = uri.trim();
    let without_scheme = trimmed
        .strip_prefix("s3://")
        .ok_or_else(|| anyhow::anyhow!("Unsupported artifact ref '{trimmed}', expected s3://"))?;
    let mut parts = without_scheme.splitn(2, '/');
    let bucket = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing bucket in artifact ref '{trimmed}'"))?;
    let key = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing object key in artifact ref '{trimmed}'"))?;
    Ok((bucket.to_string(), key.to_string()))
}

fn s3_uri(bucket: &str, object_key: &str) -> String {
    format!(
        "s3://{}/{}",
        bucket.trim_matches('/'),
        object_key.trim_start_matches('/')
    )
}

fn normalize_prefix(prefix: &str) -> String {
    prefix
        .trim()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn join_object_key(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.trim_start_matches('/').to_string()
    } else {
        format!(
            "{}/{}",
            prefix.trim_matches('/'),
            suffix.trim_start_matches('/')
        )
    }
}

pub fn summarize_text_for_event(text: &str) -> String {
    let first_non_empty = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if first_non_empty.chars().count() <= 280 {
        return first_non_empty.to_string();
    }
    let mut summary = first_non_empty.chars().take(277).collect::<String>();
    summary.push_str("...");
    summary
}

fn ensure_bucket_matches_config(config: &WorkerPlaneConfig, bucket: &str) -> Result<()> {
    if bucket != config.artifacts.bucket {
        anyhow::bail!(
            "Artifact ref bucket '{}' does not match configured bucket '{}'",
            bucket,
            config.artifacts.bucket
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkerPlaneConfig;

    #[test]
    fn artifact_refs_follow_s3_layout() {
        let mut config = WorkerPlaneConfig::default();
        config.enabled = true;
        config.mode = "redpanda_k8s".into();
        let refs = build_artifact_refs(&config, "agent-1", "v20260313", "req-1");

        assert_eq!(
            refs.spec_ref,
            "s3://labaclaw-artifacts/labaclaw/specs/agent-1/v20260313/agent-spec.json"
        );
        assert_eq!(
            refs.bootstrap_ref,
            "s3://labaclaw-artifacts/labaclaw/bootstrap/agent-1/req-1/request.json"
        );
        assert_eq!(
            refs.result_ref,
            "s3://labaclaw-artifacts/labaclaw/results/agent-1/req-1/result.md"
        );
    }

    #[test]
    fn summarize_text_picks_first_meaningful_line() {
        let summary = summarize_text_for_event("\n\nRESULT FOR ORCHESTRATOR\n30.0% margin");
        assert_eq!(summary, "RESULT FOR ORCHESTRATOR");
    }

    #[test]
    fn local_file_ref_normalizes_windows_style_paths() {
        let file_ref = local_file_ref(Path::new(r"C:\tmp\agent\result.md"));
        assert_eq!(file_ref, "file:///C:/tmp/agent/result.md");
    }

    #[test]
    fn summarize_text_handles_multibyte_without_panicking() {
        let text = format!("{}\nsecondary", "ż".repeat(400));
        let summary = summarize_text_for_event(&text);
        assert!(summary.ends_with("..."));
        assert!(summary.chars().count() <= 280);
    }

    #[test]
    fn parse_s3_uri_splits_bucket_and_key() {
        let (bucket, key) =
            parse_s3_uri("s3://labaclaw-artifacts/labaclaw/results/agent-1/req-1/result.md")
                .expect("s3 ref should parse");
        assert_eq!(bucket, "labaclaw-artifacts");
        assert_eq!(key, "labaclaw/results/agent-1/req-1/result.md");
    }

    #[test]
    fn worker_plane_enabled_requires_distributed_mode() {
        let mut config = WorkerPlaneConfig::default();
        assert!(!worker_plane_enabled(&config));
        config.enabled = true;
        assert!(!worker_plane_enabled(&config));
        config.mode = "redpanda_k8s".into();
        assert_eq!(
            worker_plane_enabled(&config),
            distributed_support_compiled()
        );
    }
}
