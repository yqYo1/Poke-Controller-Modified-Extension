# Poke-Controller Modified Extension — Agent Directives

## Session Directives (Auto-captured)

### Development Environment
- [2026-05-09] **All development work MUST use the Nix devShell.** Enter via `nix develop` or let direnv auto-enter with `use flake` in `.envrc`.
- [2026-05-09] **direnv is available and configured.** The `.envrc` contains `use flake`, so entering the project directory automatically loads the devShell. If direnv is blocked, run `direnv allow`.
- [2026-05-09] **Do NOT run tools directly from host system.** Always use `nix run .#<task>` or tools provided within `nix develop`. This ensures version parity with CI.
- [2026-05-09] **Python 3.14 is the target version.** Use PEP 695 type parameters and full type hints (basedpyright for checking).
- [2026-05-09] **PyO3 bindings build with `maturin develop --manifest-path rust/pokecon-pybindings/Cargo.toml`.**

### Code Quality & Formatting
- [2026-05-09] **Run `nix fmt` before every commit.** A pre-commit hook is configured via git-hooks.nix to auto-run `treefmt` on commit.
- [2026-05-09] **CI runs `nix fmt -- --ci` in check mode.** If formatting is not applied, the Lint workflow will fail.
- [2026-05-09] **All task apps are managed via nix flake:** `nix run .#test`, `nix run .#clippy`, `nix run .#check`, etc.

### Git Workflow
- [2026-05-09] **Never commit to main/develop.** Use `.worktree/<branch>/` for ghq repos. Branch off parent worktree when delegate_task changes files.
- [2026-05-09] **Clone repos with `ghq` (`~/.nix-profile/bin/ghq` v1.9.4).**

### Tauri v2 UI
- [2026-05-09] **Tauri is excluded from default Cargo workspace members** to avoid WebKit/GTK system library dependencies in CI.
- [2026-05-09] **Build Tauri via `nix run .#tauri-dev`** when needed.

### Investigation-First Approach
- [2026-05-09] **Investigate root causes thoroughly before proposing solutions.** Avoid shortcut approaches that skip proper investigation.

### Architecture Documentation
- [2026-05-13] **`architecture_report.md`** contains a comprehensive per-crate analysis covering all 38 `.rs` files across 9 crates. Read this before making cross-cutting changes.
- [2026-05-13] **`REFACTORING_PLAN.md`** contains prioritized refactoring tasks (P0–P3) with specific code locations.
- [2026-05-13] **Test gaps identified**: `src-tauri` (no tests), `pokecon-pybindings` (tests added in this session), `pokecon-serial::format/keypress/keys` (tests added in this session).
- [2026-05-13] **Integration tests** added: `pokecon-serial/tests/format_send_pipeline.rs`, `pokecon-events/tests/event_bus_integration.rs`, `pokecon-cv/tests/image_pipeline_integration.rs`.

### CI Monitoring (Mandatory)
- [2026-05-12] **After EVERY git push to origin, verify CI results on GitHub.** Local `nix run .#check` passing does NOT guarantee CI will pass (environment differences, feature flags, etc.).
- [2026-05-12] **Use `scripts/ci-watch.sh` to monitor CI after push.** This script polls GitHub Actions and blocks until all jobs complete, reporting success/failure.
- [2026-05-12] **If CI fails, fix before declaring completion.** Never tell user "CI passed" without actually checking GitHub.
- [2026-05-12] **CI watch command:** `scripts/ci-watch.sh [branch] [timeout-seconds]` (default: current branch, 600s timeout)
- [2026-05-12] **Alternative:** `gh run watch` or `gh run list --branch <branch>` to check status manually.

## Quick Reference

```bash
# Enter devShell (auto via direnv, or manual)
nix develop

# Format all files
nix fmt

# Run checks
nix run .#check

# Run tests
nix run .#test

# Run clippy
nix run .#clippy

# Build Rust + Python
nix run .#build

# Tauri dev server
nix run .#tauri-dev
```
