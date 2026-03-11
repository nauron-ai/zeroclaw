# LabaClaw

LabaClaw is a mesh-first, high-performance, distributed Rust runtime for agentic workflows.
It is maintained as a public fork of ZeroClaw and is being reshaped around the Laba mesh baseline.

> **Fork provenance and sync policy**
> LabaClaw is a public fork of ZeroClaw. The documentation and operator surface use `labaclaw`, `~/.labaclaw`, `LABACLAW_*`, `/etc/labaclaw`, and `labaclaw.service`.
> Upstream material may be pulled from ZeroClaw either 1:1 or adapted qualitatively, depending on fit with the LabaClaw mesh, distributed, and high-efficiency direction.

## Quick Start

### Install

Recommended for operators:

```bash
curl -fsSL https://raw.githubusercontent.com/nauron-ai/labaclaw/main/install.sh | bash
```

Alternative local install from crates.io:

```bash
cargo install labaclaw
```

Source checkout for contributors and runtime development:

```bash
git clone https://github.com/nauron-ai/labaclaw.git
cd labaclaw
./bootstrap.sh
```

### First Run

```bash
labaclaw gateway
```

Useful follow-up commands:

```bash
labaclaw doctor
labaclaw config show
labaclaw channel list
```

For the standalone operator dashboard, use [`labaclaw-web`](https://github.com/nauron-ai/labaclaw-web) against the runtime origin. `labaclaw` no longer embeds or serves the SPA from the binary. For cross-origin dashboard deployments, allow the dashboard origin with `gateway.dashboard_allowed_origins` or `LABACLAW_DASHBOARD_ALLOWED_ORIGINS` as documented in [docs/config-reference.md](docs/config-reference.md) and [docs/network-deployment.md](docs/network-deployment.md).

For the native Android app and JNI bridge, use [`labaclaw-android`](https://github.com/nauron-ai/labaclaw-android). This repository keeps Android support only for the headless runtime binaries, Termux, and cross-compilation workflows.

## Start Reading

- Docs hub: [docs/README.md](docs/README.md)
- Unified docs summary: [docs/SUMMARY.md](docs/SUMMARY.md)
- One-click setup: [docs/one-click-bootstrap.md](docs/one-click-bootstrap.md)
- Commands reference: [docs/commands-reference.md](docs/commands-reference.md)
- Config reference: [docs/config-reference.md](docs/config-reference.md)
- Operations runbook: [docs/operations-runbook.md](docs/operations-runbook.md)
- Network deployment: [docs/network-deployment.md](docs/network-deployment.md)

## Why LabaClaw

- Mesh-first runtime baseline for distributed agent execution.
- High-performance Rust core with low-overhead deployment on constrained hardware and small cloud nodes.
- Trait-driven architecture with swappable providers, channels, tools, memory, and runtime adapters.
- Secure-by-default operating model with explicit policies, bounded surfaces, and operational docs.

## Direction

LabaClaw is optimized for:

- fast entry into mesh-native topologies,
- high-efficiency execution on heterogeneous hardware,
- distributed coordination without heavyweight runtime assumptions,
- practical operator workflows for deployment, recovery, and maintenance.

## Migration

- Legacy OpenClaw migration guide for older upstream-compatible deployments: [docs/migration/openclaw-migration-guide.md](docs/migration/openclaw-migration-guide.md)

## Contributing

Contributor workflow and validation gates live in [CONTRIBUTING.md](CONTRIBUTING.md).
Engineering-agent defaults live in [AGENTS.md](AGENTS.md) and [CLAUDE.md](CLAUDE.md).

## License

LabaClaw remains dual-licensed under MIT or Apache-2.0.
See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
