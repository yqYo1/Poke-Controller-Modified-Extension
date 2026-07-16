# Poke-Controller Modified Extension — Agent Directives

## Session Directives (Auto-captured)

### Development Environment
- [2026-05-09] **All development work MUST use the Nix devShell.** Enter via `nix develop` or let direnv auto-enter with `use flake` in `.envrc`.
- [2026-05-09] **direnv is available and configured.** The `.envrc` contains `use flake`, so entering the project directory automatically loads the devShell. If direnv is blocked, run `direnv allow`.
- [2026-05-09] **Do NOT run tools directly from host system.** Always use `nix run .#<task>` or tools provided within `nix develop`. This ensures version parity with CI.
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
- [2026-05-12] **Use `scripts/ci-watch.sh` to monitor CI after push.** This script polls GitHub Actions and blocks until all jobs complete, reporting success/failure.
- [2026-05-12] **If CI fails, fix before declaring completion.** Never tell user "CI passed" without actually checking GitHub.
- [2026-05-12] **CI watch command:** `scripts/ci-watch.sh [branch] [timeout-seconds]` (default: current branch, 600s timeout)
- [2026-05-12] **Alternative:** `gh run watch` or `gh run list --branch <branch>` to check status manually.
- [2026-05-16] **After push, monitor CI until completion without user reminder.** Do not wait for user to prompt CI check. Automatically poll `gh run list` and fix failures immediately.

### Nix Source Filter — SPA 404 Incident
- [2026-05-20] **When adding new frontend file extensions, update `flake.nix` source filter.** The UI showed "404 Not Found" (HTTP 200) because `.svelte` files were missing from the nix source filter — SvelteKit could only build the fallback error page. All source extensions used by the build must be listed in the `filter` function (`.rs`, `.html`, `.css`, `.js`, `.jsx`, `.ts`, `.tsx`, `.svelte`, `.json`, `.svg`, `.md`, etc.).
- [2026-05-20] **Symptom:** Browser displays "404 Not Found" but HTTP status is 200. The SPA fallback serves `index.html` correctly, but the built `index.html` itself contains only SvelteKit's built-in error page due to missing source files during nix build.
- [2026-05-20] **Verification:** Compare `nix build` output vs local `npm run build` output. If the nix store build has significantly fewer chunks/CSS files, suspect missing source extensions in the filter.

## Quick Reference

```bash
# Enter devShell (auto via direnv, or manual)
nix develop

# Format all files
nix fmt

# Run checks
nix run .#check

```
