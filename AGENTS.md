# Poke-Controller Modified Extension — Agent Directives

## Session Directives (Auto-captured)

### Development Environment
- [2026-08-09] **All development work MUST use the Nix devShell.** Enter with `nix develop`, or let direnv load the same shell from `.envrc` with `use flake`.
- [2026-08-09] **direnv is available and configured.** Run `direnv allow` once for a new worktree when automatic loading is blocked.
- [2026-08-09] **Do NOT run host language runtimes, compilers, build tools, package managers, or quality tools directly.** Use tools provided inside `nix develop`, or the fixed `nix run .#<task>`, `nix fmt`, and `nix flake check` entrypoints.
- [2026-07-30] **These flake-output rules govern local work on Nix-capable development hosts.** Explicit Windows CI, packaging, and release jobs use the toolchains pinned by their workflows because native Windows Nix execution is not a supported gate.
- [2026-05-09] **Python 3.14 is the target version.** Use PEP 695 type parameters and full type hints (basedpyright for checking).

### Code Quality & Formatting
- [2026-05-09] **Run `nix fmt` before every commit.** A pre-commit hook is configured via git-hooks.nix to auto-run `treefmt` on commit.
- [2026-05-09] **CI runs `nix fmt -- --ci` in check mode.** If formatting is not applied, the Lint workflow will fail.
- [2026-05-09] **All task apps are managed via nix flake:** `nix run .#test`, `nix run .#clippy`, `nix run .#check`, etc.

### Git Workflow
- [2026-05-09] **Never commit to main/develop.** Use `.worktree/<branch>/` for ghq repos. Branch off parent worktree when delegate_task changes files.
- [2026-05-09] **Clone repos with `ghq` (`~/.nix-profile/bin/ghq` v1.9.4).**

### Investigation-First Approach
- [2026-05-09] **Investigate root causes thoroughly before proposing solutions.** Avoid shortcut approaches that skip proper investigation.

### CI Monitoring (Mandatory)
- [2026-05-12] **After EVERY git push to origin, verify CI results on GitHub.** Local `nix run .#check` passing does NOT guarantee CI will pass (environment differences, feature flags, etc.).
- [2026-07-30] **Use `nix run .#ci-watch --` to monitor CI after push.** The flake app runs `scripts/ci-watch.sh` with fixed dependencies, following workflow runs discovered for the pushed SHA until they complete and the discovered run/status set remains stable for the script's settlement window.
- [2026-05-12] **If CI fails, fix before declaring completion.** Never tell user "CI passed" without actually checking GitHub.
- [2026-07-30] **CI watch command:** `nix run .#ci-watch -- [branch] [timeout-seconds]` (the script default is current branch and its documented timeout).
- [2026-07-30] **After push, monitor discovered CI runs through the settlement window without user reminder.** Until the aggregate workflow is implemented, this does not prove that no later workflow run will appear after the window. Fix every discovered failure immediately.

### Nix Source Filter — SPA 404 Incident
- [2026-05-20] **When adding new frontend file extensions, update `flake.nix` source filter.** The UI showed "404 Not Found" (HTTP 200) because `.svelte` files were missing from the nix source filter — SvelteKit could only build the fallback error page. All source extensions used by the build must be listed in the `filter` function (`.rs`, `.html`, `.css`, `.ts`, `.tsx`, `.svelte`, `.json`, `.svg`, `.md`, etc.). Project-owned frontend source and configuration must remain TypeScript rather than JavaScript.
- [2026-05-20] **Symptom:** Browser displays "404 Not Found" but HTTP status is 200. The SPA fallback serves `index.html` correctly, but the built `index.html` itself contains only SvelteKit's built-in error page due to missing source files during nix build.
- [2026-05-20] **Verification:** Build the package with `nix build .#web` and run the independent `nix run .#web-check` gate. If the Nix store output contains only the fallback page or unexpectedly few chunks/CSS files, suspect missing source extensions in the filter.

## Quick Reference

```bash
# Format all files
nix fmt

# Run checks
nix run .#check

# Run a targeted Cargo command in the caller worktree
nix run .#cargo -- test --locked -p pokecon

# Install the Nix-generated pre-commit hook explicitly
nix run .#hooks-install

# Monitor GitHub Actions with fixed dependencies
nix run .#ci-watch --

```
