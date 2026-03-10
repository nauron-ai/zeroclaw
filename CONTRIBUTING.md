# Contributing to LabaClaw

Thanks for contributing to LabaClaw.
This repository is a public fork of ZeroClaw and is being actively redirected toward a mesh-first, distributed runtime.

> **Fork provenance and sync policy**
> Contributor docs use the target LabaClaw surface now.
> Some runtime and script internals still carry legacy `zeroclaw` identifiers and will be migrated in a follow-up code track.
> Upstream material may be synced from ZeroClaw either 1:1 or adapted qualitatively when LabaClaw needs a different operator or architecture shape.

## First-Time Contributors

1. Find an issue or propose a narrow, reversible improvement.
2. Start with a low-risk track when possible: docs, tests, or tightly scoped fixes.
3. Create a feature branch from a clean branch tip.
4. Run the relevant validation gates before you open a PR.

## Development Setup

```bash
git clone https://github.com/nauron-ai/labaclaw.git
cd labaclaw

git config core.hooksPath .githooks

cargo build
cargo test --locked

./scripts/ci/rust_quality_gate.sh
./scripts/ci/docs_quality_gate.sh
./scripts/ci/docs_links_gate.sh
```

Release build:

```bash
cargo build --release --locked
```

## Secret And Config Handling

Use the target LabaClaw naming in docs and examples:

- config directory: `~/.labaclaw/`
- primary config file: `~/.labaclaw/config.toml`
- secret key file: `~/.labaclaw/.secret_key`
- generic API key env var: `LABACLAW_API_KEY`
- provider/model overrides: `LABACLAW_PROVIDER`, `LABACLAW_MODEL`

Do not commit:

- `.env` files,
- API keys or tokens,
- webhook secrets,
- local secret-key files,
- personal identifiers in fixtures or examples.

## Review Tracks

| Track | Typical scope | Required review depth |
|---|---|---|
| **Track A** | docs, tests, chore, isolated low-risk refactors | 1 review + green CI |
| **Track B** | providers, channels, memory, tools, or config behavior | subsystem-aware review + explicit validation evidence |
| **Track C** | `src/security/**`, `src/runtime/**`, `src/gateway/**`, `.github/workflows/**` | deep review, validation evidence, rollback plan |

When in doubt, choose the higher track.

## Docs Rules

- Keep docs English-only.
- Use the target LabaClaw surface in user-facing docs.
- Limit ZeroClaw references to provenance, migration-status, and sync-policy notes.
- Update the nearest docs index and `docs/SUMMARY.md` when adding a major document.

## Before Opening A PR

- `cargo test --locked`
- `./scripts/ci/rust_quality_gate.sh`
- `./scripts/ci/docs_quality_gate.sh`
- `./scripts/ci/docs_links_gate.sh`
- concise PR summary with scope, validation, and any follow-up runtime rename work
