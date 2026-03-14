# LabaClaw Release Process

This runbook defines the binary-only release flow for this fork.

## Scope

- No Docker publish path.
- Release artifacts are distributed via GitHub Releases.
- Stable release tags use `vX.Y.Z`.

## Workflow Contract

- `.github/workflows/release-build.yml`
  - Builds canonical Linux binary with production release features and uploads workflow artifact.
- `.github/workflows/pub-release.yml`
  - Verify mode (manual/schedule) and publish mode (tag push or manual publish).

Production release assets are expected to include `worker-plane-distributed`, because
the host `labaclaw` binary is the component that publishes spawn/task commands to
Redpanda and uploads agent artifacts to RustFS.

Publish-mode guardrails:

- Stable tag format `vX.Y.Z`.
- Tag exists on origin and is reachable from `origin/main`.
- Annotated tag required.
- Actor allowlist enforced via `RELEASE_AUTHORIZED_ACTORS`.
- Optional tagger allowlist via `RELEASE_AUTHORIZED_TAGGER_EMAILS`.
- Artifact contract must pass before release publish.

## Maintainer Procedure

### 1) Preflight on `main`

1. Ensure required checks are green.
2. Confirm no release-blocking incidents.

### 2) Run verification build (no publish)

Run `Pub Release` manually with:

- `publish_release=false`
- `release_ref=main`

Expected:

- Target matrix builds successfully.
- `verify-artifacts` passes.
- No GitHub Release publish.

### 3) Cut stable release tag

From synced `origin/main`:

```bash
scripts/release/cut_release_tag.sh vX.Y.Z --push
```

### 4) Monitor publish run

After tag push, monitor:

- `Pub Release`
- `Production Release Build`

Expected outputs:

- Release archives
- `SHA256SUMS`
- SBOM/provenance/signature artifacts produced by release workflow
- GitHub Release assets and notes

### 5) Post-release validation

1. Verify release assets are downloadable.
2. Verify versioned binary starts and reports expected version.
3. Verify installer/bootstrap paths that consume release assets.

## Emergency Path

If publish fails after validation:

1. Fix issue on `main`.
2. Re-run `Pub Release` manually in publish mode for the existing tag.
3. Re-validate assets.
