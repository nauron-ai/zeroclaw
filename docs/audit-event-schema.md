# CI Audit Event Schema

This document defines the normalized audit event envelope used by active release workflows.

## Envelope

All audit events emitted by `scripts/ci/emit_audit_event.py` follow:

```json
{
  "schema_version": "labaclaw.audit.v1",
  "event_type": "string",
  "generated_at": "RFC3339 timestamp",
  "run_context": {
    "repository": "owner/repo",
    "workflow": "workflow name",
    "run_id": "GitHub run id",
    "run_attempt": "GitHub run attempt",
    "sha": "commit sha",
    "ref": "git ref",
    "actor": "trigger actor"
  },
  "artifact": {
    "name": "artifact name",
    "retention_days": 14
  },
  "payload": {}
}
```

## Active Event Types

- `release_trigger_guard`
- `release_artifact_guard_verify`
- `release_artifact_guard_publish`
- `release_sha256sums_provenance`

## Governance

- Keep schema stable and additive.
- Keep artifact naming deterministic.
- Document retention changes in this file and `docs/ci-map.md`.
