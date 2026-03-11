# LabaClaw Documentation Inventory

This inventory classifies maintained documentation by intent and canonical location.

Last reviewed: **March 10, 2026**.

## Classification Legend

- **Current Guide/Reference**: intended to describe the target LabaClaw operator surface
- **Policy/Process**: contribution or governance contract
- **Proposal/Roadmap**: exploratory or planned behavior
- **Snapshot/Audit**: time-bound status or audit history retained only when still operationally relevant

## Entry Points

| Doc | Type | Audience |
|---|---|---|
| `README.md` | Current Guide | all readers |
| `docs/README.md` | Current Guide (hub) | all readers |
| `docs/SUMMARY.md` | Current Guide (TOC) | all readers |
| `docs/structure/README.md` | Current Guide (structure map) | maintainers |
| `docs/structure/by-function.md` | Current Guide (function map) | maintainers/operators |

## Collection Index Docs

| Doc | Type | Audience |
|---|---|---|
| `docs/getting-started/README.md` | Current Guide | new users |
| `docs/reference/README.md` | Current Guide | users/operators |
| `docs/operations/README.md` | Current Guide | operators |
| `docs/security/README.md` | Current Guide | operators/contributors |
| `docs/hardware/README.md` | Current Guide | hardware builders |
| `docs/contributing/README.md` | Current Guide | contributors/reviewers |
| `docs/sop/README.md` | Current Guide | operators/automation maintainers |

## Current Guides And References

| Doc | Type | Audience |
|---|---|---|
| `docs/one-click-bootstrap.md` | Current Guide | users/operators |
| `docs/android-setup.md` | Current Guide | Android users/operators |
| `docs/commands-reference.md` | Current Reference | users/operators |
| `docs/providers-reference.md` | Current Reference | users/operators |
| `docs/channels-reference.md` | Current Reference | users/operators |
| `docs/config-reference.md` | Current Reference | operators |
| `docs/custom-providers.md` | Current Integration Guide | integration developers |
| `docs/zai-glm-setup.md` | Current Provider Setup Guide | users/operators |
| `docs/langgraph-integration.md` | Current Integration Guide | integration developers |
| `docs/proxy-agent-playbook.md` | Current Operations Playbook | operators/maintainers |
| `docs/operations-runbook.md` | Current Guide | operators |
| `docs/operations/connectivity-probes-runbook.md` | Current CI/ops Runbook | maintainers/operators |
| `docs/troubleshooting.md` | Current Guide | users/operators |
| `docs/network-deployment.md` | Current Guide | operators |
| `docs/mattermost-setup.md` | Current Guide | operators |
| `docs/nextcloud-talk-setup.md` | Current Guide | operators |
| `docs/migration/openclaw-migration-guide.md` | Current Migration Guide | adopters/integrators |
| `docs/cargo-slicer-speedup.md` | Current Build/CI Guide | maintainers |
| `docs/adding-boards-and-tools.md` | Current Guide | hardware builders |
| `docs/arduino-uno-q-setup.md` | Current Guide | hardware builders |
| `docs/nucleo-setup.md` | Current Guide | hardware builders |
| `docs/hardware-peripherals-design.md` | Current Design Spec | hardware contributors |
| `docs/datasheets/README.md` | Current Hardware Index | hardware builders |
| `docs/datasheets/nucleo-f401re.md` | Current Hardware Reference | hardware builders |
| `docs/datasheets/arduino-uno.md` | Current Hardware Reference | hardware builders |
| `docs/datasheets/esp32.md` | Current Hardware Reference | hardware builders |
| `docs/audit-event-schema.md` | Current CI/Security Reference | maintainers/security reviewers |
| `docs/security/official-channels-and-fraud-prevention.md` | Current Security Guide | users/operators |

## Policy And Process Docs

| Doc | Type |
|---|---|
| `docs/pr-workflow.md` | Policy |
| `docs/reviewer-playbook.md` | Process |
| `docs/ci-map.md` | Process |
| `docs/actions-source-policy.md` | Policy |
| `CONTRIBUTING.md` | Process |
| `AGENTS.md` | Process |
| `CLAUDE.md` | Process |

## Proposal And Roadmap Docs

These are useful context, but not strict runtime contracts.

| Doc | Type |
|---|---|
| `docs/sandboxing.md` | Proposal |
| `docs/resource-limits.md` | Proposal |
| `docs/audit-logging.md` | Proposal |
| `docs/agnostic-security.md` | Proposal |
| `docs/frictionless-security.md` | Proposal |
| `docs/security-roadmap.md` | Roadmap |

## Maintenance Contract

1. Update `docs/SUMMARY.md` and the nearest category index when adding a major doc.
2. Keep the documentation English-only and use the current LabaClaw surface in user-facing docs.
3. Limit ZeroClaw mentions to fork provenance and upstream sync policy.
4. Call out unresolved runtime or script gaps anywhere they affect setup, operations, or migration guidance.
5. Remove stale planning and project-tracking docs instead of carrying them forward once they stop serving the active LabaClaw direction.
