# One-Click Bootstrap

This page defines the fastest supported path to install and initialize LabaClaw.

Last verified: **March 4, 2026**.

## Option A (Recommended): Clone + local script

```bash
git clone https://github.com/nauron-ai/labaclaw.git
cd labaclaw
./bootstrap.sh
```

What it does by default:

1. On Linux, interactive no-flag runs enter the guided installer before build/install.
2. The guided flow can install system dependencies and Rust, choose build vs prebuilt, choose whether to install, and optionally run onboarding.
3. Outside guided mode, bootstrap uses the standard local build/install flow and can launch interactive onboarding when selected.

### Resource preflight and pre-built flow

Source builds typically require at least:

- **2 GB RAM + swap**
- **6 GB free disk**

When resources are constrained, bootstrap now attempts a pre-built binary first.

```bash
./bootstrap.sh --prefer-prebuilt
```

To require binary-only installation and fail if no compatible release asset exists:

```bash
./bootstrap.sh --prebuilt-only
```

To bypass pre-built flow and force source compilation:

```bash
./bootstrap.sh --force-source-build
```

## Dual-mode bootstrap

On Linux, no-flag interactive runs prefer the guided installer. Outside that path, bootstrap uses the standard build/install flow and interactive onboarding when selected.
It still expects an existing Rust toolchain unless you enable bootstrap flags below.

For fresh machines, enable environment bootstrap explicitly:

```bash
./bootstrap.sh --install-system-deps --install-rust
```

Notes:

- `--install-system-deps` installs compiler/build prerequisites (may require `sudo`).
- `--install-rust` installs Rust via `rustup` when missing.
- `--prefer-prebuilt` tries release binary download first, then falls back to source build.
- `--prebuilt-only` disables source fallback.
- `--force-source-build` disables pre-built flow entirely.

## Option B: Remote one-liner

```bash
curl -fsSL https://raw.githubusercontent.com/nauron-ai/labaclaw/main/install.sh | bash
```

Equivalent GitHub-hosted installer entrypoint:

```bash
curl -fsSL https://raw.githubusercontent.com/nauron-ai/labaclaw/main/install.sh | bash
```

For high-security environments, prefer Option A so you can review the script before execution.

No-arg interactive runs on Linux default to the guided installer, which can continue into onboarding.

Legacy compatibility:

```bash
curl -fsSL https://raw.githubusercontent.com/nauron-ai/labaclaw/main/scripts/install.sh | bash
```

This legacy endpoint prefers `labaclaw_install.sh`, then `scripts/bootstrap.sh`, and otherwise errors if neither entrypoint exists in that revision.

If you run Option B outside a repository checkout, the bootstrap script automatically clones a temporary workspace, builds, installs, and then cleans it up.

## Optional onboarding modes

### Containerized onboarding (Docker)

```bash
./bootstrap.sh --docker
```

This builds a local LabaClaw image and launches onboarding inside a container while
persisting config/workspace to `./.labaclaw-docker`.

Container CLI defaults to `docker`. If Docker CLI is unavailable and `podman` exists,
bootstrap auto-falls back to `podman`. You can also set `LABACLAW_CONTAINER_CLI`
explicitly (for example: `LABACLAW_CONTAINER_CLI=podman ./bootstrap.sh --docker`).

For Podman, bootstrap runs with `--userns keep-id` and `:Z` volume labels so
workspace/config mounts remain writable inside the container.

If you add `--skip-build`, bootstrap skips local image build. It first tries the local
Docker tag (`LABACLAW_DOCKER_IMAGE`, default: `labaclaw-bootstrap:local`); if missing,
it pulls `ghcr.io/nauron-ai/labaclaw:latest` and tags it locally before running.

### Quick onboarding (non-interactive)

```bash
./bootstrap.sh --onboard --api-key "sk-..." --provider openrouter
```

Or with environment variables:

```bash
LABACLAW_API_KEY="sk-..." LABACLAW_PROVIDER="openrouter" ./bootstrap.sh --onboard
```

### Interactive onboarding

```bash
./bootstrap.sh --interactive-onboard
```

This launches the full-screen TUI onboarding flow (`labaclaw onboard --interactive-ui`).

## Useful flags

- `--install-system-deps`
- `--install-rust`
- `--skip-build` (in `--docker` mode: use local image if present, otherwise pull `ghcr.io/nauron-ai/labaclaw:latest`)
- `--skip-install`
- `--provider <id>`

See all options:

```bash
./bootstrap.sh --help
```

## Related docs

- [README.md](../README.md)
- [commands-reference.md](commands-reference.md)
- [providers-reference.md](providers-reference.md)
- [channels-reference.md](channels-reference.md)
