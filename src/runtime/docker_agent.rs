use crate::config::DockerRuntimeConfig;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const DEFAULT_CONTAINER_CONFIG_DIR: &str = "/agent";
const DOCKER_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceMount {
    pub host_path: PathBuf,
    pub container_path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct DockerAgentSpawnRequest {
    pub container_name: String,
    pub image: String,
    pub host_config_dir: PathBuf,
    pub container_config_dir: String,
    pub workspace_mounts: Vec<AgentWorkspaceMount>,
    pub env: HashMap<String, String>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerAgentServiceStatus {
    pub container_id: String,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct DockerAgentSpawner {
    config: DockerRuntimeConfig,
}

impl DockerAgentSpawner {
    pub fn new(config: DockerRuntimeConfig) -> Self {
        Self { config }
    }

    fn parse_inspect_output(
        &self,
        container_name: &str,
        raw_output: &str,
    ) -> Result<DockerAgentServiceStatus> {
        let trimmed = raw_output.trim();
        let mut parts = trimmed.splitn(2, '|');
        let container_id = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .with_context(|| {
                format!(
                    "Malformed docker inspect output for '{}': missing container id in {:?}",
                    container_name, raw_output
                )
            })?;
        let state = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .with_context(|| {
                format!(
                    "Malformed docker inspect output for '{}': missing container state in {:?}",
                    container_name, raw_output
                )
            })?;

        Ok(DockerAgentServiceStatus {
            container_id: container_id.to_string(),
            state: state.to_string(),
        })
    }

    fn validate_host_path(&self, path: &Path) -> Result<PathBuf> {
        let resolved = path.canonicalize().with_context(|| {
            format!(
                "Host path {} does not exist or is inaccessible",
                path.display()
            )
        })?;
        if !resolved.is_absolute() {
            anyhow::bail!(
                "Docker agent spawner requires an absolute path, got {}",
                resolved.display()
            );
        }
        if resolved == Path::new("/") {
            anyhow::bail!("Refusing to mount filesystem root (/) into spawned agent container");
        }
        Ok(resolved)
    }

    fn normalize_container_path(&self, raw_path: &str, field_name: &str) -> Result<String> {
        let normalized = raw_path.trim();
        if normalized.is_empty() {
            anyhow::bail!("{field_name} must not be empty");
        }

        let path = Path::new(normalized);
        if !path.is_absolute() {
            anyhow::bail!("{field_name} must be an absolute container path");
        }
        if path == Path::new("/") {
            anyhow::bail!("{field_name} must not be filesystem root (/)");
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            anyhow::bail!("{field_name} must not contain '.' or '..' segments");
        }

        Ok(normalized.to_string())
    }

    fn validate_workspace_mount_path(&self, path: &Path) -> Result<PathBuf> {
        if !self.config.mount_workspace {
            anyhow::bail!("Workspace mounts are disabled in runtime.docker.mount_workspace");
        }

        let resolved = self.validate_host_path(path)?;
        if self.config.allowed_workspace_roots.is_empty() {
            return Ok(resolved);
        }

        let mut allowed = false;
        for root in &self.config.allowed_workspace_roots {
            let root_path = Path::new(root).canonicalize().with_context(|| {
                format!(
                    "Failed to resolve runtime.docker.allowed_workspace_roots entry {}",
                    root
                )
            })?;
            if resolved.starts_with(&root_path) {
                allowed = true;
                break;
            }
        }

        if !allowed {
            anyhow::bail!(
                "Workspace path {} is not in runtime.docker.allowed_workspace_roots",
                resolved.display()
            );
        }

        Ok(resolved)
    }

    pub fn default_container_config_dir() -> &'static str {
        DEFAULT_CONTAINER_CONFIG_DIR
    }

    pub fn build_spawn_command(&self, request: &DockerAgentSpawnRequest) -> Result<Command> {
        let mut process = Command::new("docker");
        let container_name = request.container_name.trim();
        if container_name.is_empty() {
            anyhow::bail!("spawned agent container_name must not be empty");
        }
        process
            .arg("run")
            .arg("--detach")
            .arg("--init")
            .arg("--name")
            .arg(container_name);

        let image = request.image.trim();
        if image.is_empty() {
            anyhow::bail!("spawned agent image must not be empty");
        }

        let network = self.config.network.trim();
        if !network.is_empty() {
            process.arg("--network").arg(network);
        }

        if let Some(memory_limit_mb) = self.config.memory_limit_mb.filter(|mb| *mb > 0) {
            process.arg("--memory").arg(format!("{memory_limit_mb}m"));
        }

        if let Some(cpu_limit) = self.config.cpu_limit.filter(|cpus| *cpus > 0.0) {
            process.arg("--cpus").arg(cpu_limit.to_string());
        }

        if self.config.read_only_rootfs {
            process.arg("--read-only");
        }

        process.arg("--tmpfs").arg("/tmp:rw,size=64m");

        let host_config_dir = self
            .validate_host_path(&request.host_config_dir)
            .with_context(|| {
                format!(
                    "Failed to validate spawned agent config dir {}",
                    request.host_config_dir.display()
                )
            })?;
        let container_config_dir = self.normalize_container_path(
            &request.container_config_dir,
            "spawned agent container_config_dir",
        )?;
        process.arg("--volume").arg(format!(
            "{}:{}:rw",
            host_config_dir.display(),
            container_config_dir
        ));

        for mount in &request.workspace_mounts {
            let host_path = self
                .validate_workspace_mount_path(&mount.host_path)
                .with_context(|| {
                    format!(
                        "Failed to validate workspace mount {}",
                        mount.host_path.display()
                    )
                })?;
            let container_path = self.normalize_container_path(
                &mount.container_path,
                "spawned agent workspace container_path",
            )?;
            let mode = if mount.read_only { "ro" } else { "rw" };
            process.arg("--volume").arg(format!(
                "{}:{}:{}",
                host_path.display(),
                container_path,
                mode
            ));
        }

        process
            .arg("--env")
            .arg(format!("LABACLAW_CONFIG_DIR={}", container_config_dir));

        for (key, value) in &request.env {
            process.arg("--env").arg(format!("{key}={value}"));
        }

        for (key, value) in &request.labels {
            process.arg("--label").arg(format!("{key}={value}"));
        }

        process
            .arg(image)
            .arg("--config-dir")
            .arg(&container_config_dir)
            .arg("agent-runtime")
            .arg("--poll-interval-ms")
            .arg("1000");

        Ok(process)
    }

    async fn run_docker_output(
        &self,
        mut command: Command,
        description: &str,
    ) -> Result<std::process::Output> {
        command.kill_on_drop(true);
        timeout(DOCKER_COMMAND_TIMEOUT, command.output())
            .await
            .with_context(|| {
                format!(
                    "Timed out after {}s while {}",
                    DOCKER_COMMAND_TIMEOUT.as_secs(),
                    description
                )
            })?
            .with_context(|| format!("Failed to {description}"))
    }

    pub async fn spawn_service(&self, request: &DockerAgentSpawnRequest) -> Result<String> {
        let command = self.build_spawn_command(request)?;
        let output = self
            .run_docker_output(command, "execute docker run for spawned agent")
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Failed to start spawned agent container '{}': {}",
                request.container_name,
                stderr.trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub async fn inspect_service(
        &self,
        container_name: &str,
    ) -> Result<Option<DockerAgentServiceStatus>> {
        let mut command = Command::new("docker");
        command
            .arg("inspect")
            .arg("--format")
            .arg("{{.Id}}|{{.State.Status}}")
            .arg(container_name);
        let output = self
            .run_docker_output(command, "inspect spawned agent container")
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let trimmed = stderr.trim();
            if trimmed.contains("No such object")
                || trimmed.contains("No such container")
                || trimmed.contains("not found")
            {
                return Ok(None);
            }
            anyhow::bail!(
                "Failed to inspect spawned agent container '{}': {}",
                container_name,
                trimmed
            );
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let parsed = self
            .parse_inspect_output(container_name, raw.as_ref())
            .with_context(|| {
                format!(
                    "Docker inspect for '{}' exited successfully but returned malformed output (status: {})",
                    container_name, output.status
                )
            })?;

        Ok(Some(parsed))
    }

    pub async fn stop_service(&self, container_name: &str) -> Result<()> {
        self.run_simple_docker_command(&["stop", container_name], "stop")
            .await
    }

    pub async fn start_service(&self, container_name: &str) -> Result<()> {
        self.run_simple_docker_command(&["start", container_name], "start")
            .await
    }

    pub async fn terminate_service(&self, container_name: &str) -> Result<()> {
        self.run_simple_docker_command(&["rm", "-f", container_name], "terminate")
            .await
    }

    async fn run_simple_docker_command(&self, args: &[&str], action: &str) -> Result<()> {
        let mut command = Command::new("docker");
        command.args(args);
        let output = self
            .run_docker_output(command, &format!("execute docker {action} command"))
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Failed to {action} spawned agent container: {}",
                stderr.trim()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spawner() -> DockerAgentSpawner {
        DockerAgentSpawner::new(DockerRuntimeConfig {
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            network: "bridge".into(),
            memory_limit_mb: Some(768),
            cpu_limit: Some(2.0),
            read_only_rootfs: true,
            mount_workspace: true,
            allowed_workspace_roots: Vec::new(),
        })
    }

    #[test]
    fn build_spawn_command_contains_expected_runtime_flags() {
        let spawner = test_spawner();
        let request = DockerAgentSpawnRequest {
            container_name: "agent-finance-123".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: std::env::temp_dir(),
            container_config_dir: "/agent".into(),
            workspace_mounts: vec![AgentWorkspaceMount {
                host_path: std::env::temp_dir(),
                container_path: "/mounted-workspaces/0".into(),
                read_only: true,
            }],
            env: HashMap::from([(String::from("FOO"), String::from("bar"))]),
            labels: HashMap::from([(
                String::from("labaclaw.agent_id"),
                String::from("agent-finance-123"),
            )]),
        };

        let command = spawner.build_spawn_command(&request).unwrap();
        let debug = format!("{command:?}");

        assert!(debug.contains("docker"));
        assert!(debug.contains("--detach"));
        assert!(debug.contains("--name"));
        assert!(debug.contains("agent-finance-123"));
        assert!(debug.contains("--memory"));
        assert!(debug.contains("768m"));
        assert!(debug.contains("--cpus"));
        assert!(debug.contains('2'));
        assert!(debug.contains("--read-only"));
        assert!(debug.contains("LABACLAW_CONFIG_DIR=/agent"));
        assert!(debug.contains("agent-runtime"));
        assert!(debug.contains("--poll-interval-ms"));
    }

    #[test]
    fn spawn_builder_refuses_root_mount() {
        let spawner = test_spawner();
        let request = DockerAgentSpawnRequest {
            container_name: "agent-root".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: PathBuf::from("/"),
            container_config_dir: "/agent".into(),
            workspace_mounts: Vec::new(),
            env: HashMap::new(),
            labels: HashMap::new(),
        };

        let result = spawner.build_spawn_command(&request);
        assert!(result.is_err());
    }

    #[test]
    fn spawn_builder_rejects_missing_relative_config_dir() {
        let spawner = test_spawner();
        let request = DockerAgentSpawnRequest {
            container_name: "agent-missing".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: PathBuf::from("./does-not-exist"),
            container_config_dir: "/agent".into(),
            workspace_mounts: Vec::new(),
            env: HashMap::new(),
            labels: HashMap::new(),
        };

        let error = spawner
            .build_spawn_command(&request)
            .expect_err("must reject");
        let message = error.to_string();
        assert!(message.contains("Failed to validate spawned agent config dir"));
        assert!(message.contains("does-not-exist"));
    }

    #[test]
    fn spawn_builder_rejects_workspace_mounts_when_disabled() {
        let spawner = DockerAgentSpawner::new(DockerRuntimeConfig {
            mount_workspace: false,
            ..test_spawner().config
        });
        let request = DockerAgentSpawnRequest {
            container_name: "agent-workspace-disabled".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: std::env::temp_dir(),
            container_config_dir: "/agent".into(),
            workspace_mounts: vec![AgentWorkspaceMount {
                host_path: std::env::temp_dir(),
                container_path: "/workspace".into(),
                read_only: true,
            }],
            env: HashMap::new(),
            labels: HashMap::new(),
        };

        let error = spawner
            .build_spawn_command(&request)
            .expect_err("workspace mounts must be rejected");
        let message = error.to_string();
        assert!(message.contains("Failed to validate workspace mount"));
        assert!(message.contains(&std::env::temp_dir().display().to_string()));
    }

    #[test]
    fn spawn_builder_rejects_empty_container_name() {
        let spawner = test_spawner();
        let request = DockerAgentSpawnRequest {
            container_name: "   ".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: std::env::temp_dir(),
            container_config_dir: "/agent".into(),
            workspace_mounts: Vec::new(),
            env: HashMap::new(),
            labels: HashMap::new(),
        };

        let error = spawner
            .build_spawn_command(&request)
            .expect_err("empty container name must be rejected");
        assert!(error
            .to_string()
            .contains("spawned agent container_name must not be empty"));
    }

    #[test]
    fn spawn_builder_rejects_invalid_allowed_workspace_root() {
        let spawner = DockerAgentSpawner::new(DockerRuntimeConfig {
            allowed_workspace_roots: vec!["./definitely-missing-root".into()],
            ..test_spawner().config
        });
        let request = DockerAgentSpawnRequest {
            container_name: "agent-bad-root".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: std::env::temp_dir(),
            container_config_dir: "/agent".into(),
            workspace_mounts: vec![AgentWorkspaceMount {
                host_path: std::env::temp_dir(),
                container_path: "/workspace".into(),
                read_only: true,
            }],
            env: HashMap::new(),
            labels: HashMap::new(),
        };

        let error = spawner
            .build_spawn_command(&request)
            .expect_err("invalid allowed root must be rejected");
        let message = format!("{error:#}");
        assert!(message.contains("Failed to resolve runtime.docker.allowed_workspace_roots entry"));
    }

    #[test]
    fn build_spawn_command_normalizes_container_paths_once() {
        let spawner = test_spawner();
        let request = DockerAgentSpawnRequest {
            container_name: "agent-normalized".into(),
            image: "ghcr.io/nauron-ai/labaclaw:dev".into(),
            host_config_dir: std::env::temp_dir(),
            container_config_dir: " /agent ".into(),
            workspace_mounts: vec![AgentWorkspaceMount {
                host_path: std::env::temp_dir(),
                container_path: " /mounted-workspaces/0 ".into(),
                read_only: true,
            }],
            env: HashMap::new(),
            labels: HashMap::new(),
        };

        let command = spawner.build_spawn_command(&request).expect("must build");
        let debug = format!("{command:?}");

        assert!(debug.contains("LABACLAW_CONFIG_DIR=/agent"));
        assert!(debug.contains("\"/agent\""));
        assert!(!debug.contains(" /agent "));
        assert!(!debug.contains(" /mounted-workspaces/0 "));
    }

    #[test]
    fn parse_inspect_output_rejects_empty_payload() {
        let spawner = test_spawner();
        let error = spawner
            .parse_inspect_output("agent-empty", "")
            .expect_err("empty output must fail");

        assert!(error
            .to_string()
            .contains("Malformed docker inspect output for 'agent-empty'"));
    }

    #[test]
    fn parse_inspect_output_rejects_missing_state() {
        let spawner = test_spawner();
        let error = spawner
            .parse_inspect_output("agent-missing-state", "container-id|")
            .expect_err("missing state must fail");

        assert!(error.to_string().contains("missing container state"));
    }
}
