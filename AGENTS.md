# AGENTS.md — LabaClaw Engineering Protocol

This file defines the default working protocol for coding agents in this repository.
Scope: entire repository.

## Repository Target

- Treat this repository as the active coding target unless the operator explicitly redirects scope.
- Verify the current working tree before mutating files.
- Prefer small, reversible changes with explicit validation.

## Project Snapshot

LabaClaw is a Rust-first agent runtime optimized for:

- mesh-first execution,
- high performance and high efficiency,
- distributed deployment across heterogeneous hardware,
- modular extension through traits and factory wiring,
- secure operational defaults.

## Core Extension Points

- `src/providers/traits.rs`
- `src/channels/traits.rs`
- `src/tools/traits.rs`
- `src/memory/traits.rs`
- `src/observability/traits.rs`
- `src/runtime/traits.rs`
- `src/peripherals/traits.rs`

## Engineering Rules

- Prefer trait implementations and factory registration over cross-cutting rewrites.
- Treat `src/config/**`, CLI wiring, and docs references as public contract surfaces.
- Default to fail-fast behavior, explicit errors, and reversible patches.
- Avoid hidden coupling and speculative abstraction.
- Keep validation proportional to risk.

## Documentation Contract

- English is the only maintained documentation language in this repository.
- User-facing docs should use the target LabaClaw surface and naming now.
- ZeroClaw may appear only in:
  - fork provenance notes,
  - runtime migration status notes,
  - upstream sync policy notes.
- Upstream documentation may be brought in from ZeroClaw either 1:1 or adapted qualitatively when the mesh-first LabaClaw direction requires it.
- Keep `README.md`, `docs/README.md`, `docs/SUMMARY.md`, and `docs/docs-inventory.md` aligned when navigation changes.

## Validation Expectations

- Docs-only changes: run docs quality and docs links gates.
- Runtime/config/CLI changes: run the relevant Rust quality and test gates in addition to docs updates.
- Do not leave dead links, broken navigation, or stale user-facing names after a rename pass.
