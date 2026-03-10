# LabaClaw Docs Structure Map

This page defines the canonical English-only documentation layout.

Last refreshed: **March 10, 2026**.

## Directory Spine

### Layer A: entry points

- Root overview: `README.md`
- Docs hub: `docs/README.md`
- Unified TOC: `docs/SUMMARY.md`
- Function-oriented map: `docs/structure/by-function.md`

### Layer B: category collections

- `docs/getting-started/`
- `docs/reference/`
- `docs/operations/`
- `docs/security/`
- `docs/hardware/`
- `docs/contributing/`
- `docs/sop/`
- `docs/migration/`

### Layer C: category and standalone docs

- task-first setup guides live at docs root or in `docs/getting-started/`
- runtime contract references live in canonical English docs
- snapshots and RFIs stay date-stamped and immutable after publication
- archived project notes may live under `docs/project/`, but they are not part of the active category spine

## Placement Rules

1. English is the only maintained documentation language in this repository.
2. User-facing docs should use the target LabaClaw surface and naming.
3. ZeroClaw may appear only in:
   - fork provenance statements,
   - runtime migration status notes,
   - upstream sync policy notes.
4. Every new major doc should be linked from:
   - the nearest category index,
   - `docs/SUMMARY.md`,
   - `docs/docs-inventory.md`.
5. Historical snapshots can keep historical analysis, but should not keep dead navigation links to removed locale trees or deleted shims.
6. Do not add active project-tracking or Linear workflow entry points to the docs spine while planning migrates to the new system.

## Governance Notes

- The documentation system is English-only.
- Upstream ZeroClaw content may be pulled either 1:1 or adapted qualitatively, depending on fit with the LabaClaw mesh-first and distributed direction.
- Docs are allowed to lead the runtime rename in this phase; the code/runtime rename is a separate follow-up track.

## Companion Indexes

- Hub entry point: [../README.md](../README.md)
- Unified TOC: [../SUMMARY.md](../SUMMARY.md)
- Function-oriented map: [by-function.md](by-function.md)
- Inventory and classification: [../docs-inventory.md](../docs-inventory.md)
