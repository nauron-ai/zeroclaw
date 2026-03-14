use crate::config::DockerRuntimeConfig;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DEFAULT_CONTAINER_CONFIG_DIR: &str = "/agent";

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

    pub fn default_container_config_dir() -> &'static str {
        DEFAULT_CONTAINER_CONFIG_DIR
    }

    pub fn build_spawn_command(&self, request: &DockerAgentSpawnRequest) -> Result<Command> {
        let mut process = Command::new("docker");
        process
            .arg("run")
            .arg("--detach")
            .arg("--init")
            .arg("--name")
            .arg(request.container_name.trim());

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
        process.arg("--volume").arg(format!(
            "{}:{}:rw",
            host_config_dir.display(),
            request.container_config_dir
        ));

        for mount in &request.workspace_mounts {
            let host_path = self.validate_host_path(&mount.host_path).with_context(|| {
                format!(
                    "Failed to validate workspace mount {}",
                    mount.host_path.display()
                )
            })?;
            let mode = if mount.read_only { "ro" } else { "rw" };
            process.arg("--volume").arg(format!(
                "{}:{}:{}",
                host_path.display(),
                mount.container_path,
                mode
            ));
        }

        process.arg("--env").arg(format!(
            "LABACLAW_CONFIG_DIR={}",
            request.container_config_dir.trim()
        ));

        for (key, value) in &request.env {
            process.arg("--env").arg(format!("{key}={value}"));
        }

        for (key, value) in &request.labels {
            process.arg("--label").arg(format!("{key}={value}"));
        }

        process
            .arg(image)
            .arg("--config-dir")
            .arg(request.container_config_dir.trim())
            .arg("agent-runtime")
            .arg("--poll-interval-ms")
            .arg("1000");

        Ok(process)
    }

    pub async fn spawn_service(&self, request: &DockerAgentSpawnRequest) -> Result<String> {
        let output = self
            .build_spawn_command(request)?
            .output()
            .await
            .context("Failed to execute docker run for spawned agent")?;

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
        let output = Command::new("docker")
            .arg("inspect")
            .arg("--format")
            .arg("{{.Id}}|{{.State.Status}}")
            .arg(container_name)
            .output()
            .await
            .context("Failed to inspect spawned agent container")?;

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
        let trimmed = raw.trim();
        let mut parts = trimmed.splitn(2, '|');
        let Some(container_id) = parts.next() else {
            return Ok(None);
        };
        let Some(state) = parts.next() else {
            return Ok(None);
        };

        Ok(Some(DockerAgentServiceStatus {
            container_id: container_id.trim().to_string(),
            state: state.trim().to_string(),
        }))
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
        let output = Command::new("docker")
            .args(args)
            .output()
            .await
            .with_context(|| format!("Failed to execute docker {action} command"))?;

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
        assert!(error
            .to_string()
            .contains("Host path ./does-not-exist does not exist or is inaccessible"));
    }
}
