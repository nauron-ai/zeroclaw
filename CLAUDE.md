# CLAUDE.md — LabaClaw Agent Engineering Protocol

This repository follows a compact, high-signal execution contract.

## Repository Intent

LabaClaw is a public fork of ZeroClaw being redirected toward a mesh-first, distributed, high-performance runtime.

Operator-facing defaults:

- optimize for clarity, reversibility, and low blast radius,
- preserve trait and module boundaries,
- keep public contracts explicit,
- prefer docs and validation that match the intended LabaClaw surface.

## Technical Priorities

1. Performance and efficiency are product requirements, not optional polish.
2. Security-sensitive surfaces require narrow changes and explicit validation.
3. Configuration, CLI wording, and operator docs are public interfaces.
4. Runtime evolution should favor modular traits and factory wiring.

## Docs And Naming Rules

- English-only docs are the maintained source of truth.
- Use `LabaClaw`, `labaclaw`, `~/.labaclaw`, `LABACLAW_*`, `/etc/labaclaw`, and `labaclaw.service` in user-facing docs.
- Keep ZeroClaw references limited to:
  - fork provenance,
  - runtime migration status,
  - upstream sync policy.
- Upstream docs may be synced from ZeroClaw either 1:1 or through qualitative adaptation.

## Working Style

- Read before writing.
- Keep patches small and easy to roll back.
- Prefer updating nearest docs and inventories in the same change.
- Validate links, docs quality, and affected runtime paths before closing work.
