# macOS Update and Uninstall Guide

This page documents supported update and uninstall procedures for LabaClaw on macOS (OS X).

Last verified: **February 22, 2026**.

## 1) Check current install method

```bash
which labaclaw
labaclaw --version
```

Typical locations:

- Homebrew: `/opt/homebrew/bin/labaclaw` (Apple Silicon) or `/usr/local/bin/labaclaw` (Intel)
- Cargo/bootstrap/manual: `~/.cargo/bin/labaclaw`

If both exist, your shell `PATH` order decides which one runs.

## 2) Update on macOS

Quick way to get install-method-specific guidance:

```bash
labaclaw update --instructions
labaclaw update --check
```

### A) Homebrew install

```bash
brew update
brew upgrade labaclaw
labaclaw --version
```

### B) Clone + bootstrap install

From your local repository checkout:

```bash
git pull --ff-only
./bootstrap.sh --prefer-prebuilt
labaclaw --version
```

If you want source-only update:

```bash
git pull --ff-only
cargo install --path . --force --locked
labaclaw --version
```

### C) Manual prebuilt binary install

Re-run your download/install flow with the latest release asset, then verify:

```bash
labaclaw --version
```

You can also use the built-in updater for manual/local installs:

```bash
labaclaw update
labaclaw --version
```

## 3) Uninstall on macOS

### A) Stop and remove background service first

This prevents the daemon from continuing to run after binary removal.

```bash
labaclaw service stop || true
labaclaw service uninstall || true
```

Service artifacts removed by `service uninstall`:

- `~/Library/LaunchAgents/com.labaclaw.daemon.plist`

### B) Remove the binary by install method

Homebrew:

```bash
brew uninstall labaclaw
```

Cargo/bootstrap/manual (`~/.cargo/bin/labaclaw`):

```bash
cargo uninstall labaclaw || true
rm -f ~/.cargo/bin/labaclaw
```

### C) Optional: remove local runtime data

Only run this if you want a full cleanup of config, auth profiles, logs, and workspace state.

```bash
rm -rf ~/.labaclaw
```

## 4) Verify uninstall completed

```bash
command -v labaclaw || echo "labaclaw binary not found"
pgrep -fl labaclaw || echo "No running labaclaw process"
```

If `pgrep` still finds a process, stop it manually and re-check:

```bash
pkill -f labaclaw
```

## Related docs

- [One-Click Bootstrap](../one-click-bootstrap.md)
- [Commands Reference](../commands-reference.md)
- [Troubleshooting](../troubleshooting.md)
