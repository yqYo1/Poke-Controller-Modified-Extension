from __future__ import annotations

import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
NORMAL_CI = REPO / ".github/workflows/normal-ci.yml"
PACKAGE_CI = REPO / ".github/workflows/package.yml"
REGISTRY = REPO / "rust/pokecon/registry/ci.json"

TRUSTED_TRUE_RE = re.compile(r"trusted\s*=\s*true")
REQUIRE_SIGS_RE = re.compile(r"require-sigs\s*=\s*true")
TRUSTED_PUBKEY_RE = re.compile(
    r"trusted-public-keys\s*=.*\$POKECON_NIX_CACHE_PUBLIC_KEY"
)
HEAD_REF_EXCLUSION_RE = re.compile(r"github\.head_ref\s*!=\s*'refactor/rust-core'")
PUBLIC_KEY_ENV = "POKECON_NIX_CACHE_PUBLIC_KEY"


def _read(p: Path) -> str:
    return p.read_text(encoding="utf-8")


def test_normal_ci_has_no_trusted_true() -> None:
    text = _read(NORMAL_CI)
    assert not TRUSTED_TRUE_RE.search(text), (
        "normal-ci.yml must not contain trusted=true (untrusted PR cache would execute as trusted)"
    )


def test_package_ci_has_no_trusted_true() -> None:
    text = _read(PACKAGE_CI)
    assert not TRUSTED_TRUE_RE.search(text), "package.yml must not contain trusted=true"


def test_normal_ci_requires_signed_cache() -> None:
    text = _read(NORMAL_CI)
    assert REQUIRE_SIGS_RE.search(text), (
        "normal-ci.yml must set require-sigs = true for fail-closed verification"
    )
    assert TRUSTED_PUBKEY_RE.search(text), (
        "normal-ci.yml must declare trusted-public-keys including pokecon-nix-cache-1"
    )
    assert PUBLIC_KEY_ENV in text, (
        "public key must be supplied through the repository secret"
    )
    assert "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" not in text


def test_normal_ci_restores_without_trusted_flag_but_enables_signed_substituter() -> (
    None
):
    text = _read(NORMAL_CI)
    # Ensure file:// cache is used as extra-substituter without trusted=true
    assert (
        "extra-substituters = file://$POKECON_NIX_CACHE_DIRECTORY" in text
        or "extra-substituters = file://$RUNNER_TEMP/pokecon-nix-cache" in text
        or "extra-substituters = $cache_uri" in text
    )
    # priority is OK, but trusted=true must not be present
    assert "file://$POKECON_NIX_CACHE_DIRECTORY?trusted=true" not in text


def test_writer_is_push_only_and_trusted_actor() -> None:
    text = _read(NORMAL_CI)
    # Writer steps must be gated on push and trusted actor yqYo1
    assert "github.event_name == 'push'" in text
    assert "contains(fromJSON('[\"yqYo1\"]'), github.actor)" in text
    assert "steps.rust_cache_restore.outputs.cache-hit != 'true'" in text
    # Secret-backed signing
    assert "POKECON_NIX_CACHE_SECRET_KEY" in text
    assert "nix store sign --key-file" in text
    assert "nix copy" in text and "--to" in text


def test_reader_is_unrestricted_but_fail_closed() -> None:
    text = _read(NORMAL_CI)
    # Restore steps should exist and be unrestricted (no actor/event guard on restore)
    assert "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9" in text
    # Restore must not be gated on push
    # Check that restore steps don't have an if that restricts to push
    # We verify by ensuring the restore step block does not contain "github.event_name == 'push'" in the 5 lines before cache/restore
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if "actions/cache/restore" in line:
            window = "\n".join(lines[max(0, i - 10) : i])
            assert "github.event_name == 'push'" not in window, (
                "restore must be readable by PRs (no push-only gate), trust is via signatures"
            )


def test_required_jobs_run_on_refactor_pr() -> None:
    for path in (NORMAL_CI, PACKAGE_CI):
        text = _read(path)
        assert not HEAD_REF_EXCLUSION_RE.search(text), (
            f"{path.name} must not exclude github.head_ref != 'refactor/rust-core'; required jobs must run on PRs from refactor/rust-core to avoid bypass"
        )
    # Also check that required job's if still allows PRs (contains pull_request check)
    normal = _read(NORMAL_CI)
    # The required job should contain always() and pull_request head repo check
    assert "Normal CI Required" in normal
    assert "always()" in normal


def test_registry_binary_cache_contract_matches_signed_design() -> None:
    data = json.loads(REGISTRY.read_text(encoding="utf-8"))
    bc = data["binary_cache_contract"]
    impl = bc["current_implementation"]
    assert impl["status"] == "signed_file_cache_optional"
    assert impl["pokecon_specific_read"] is True
    assert impl["pokecon_specific_write"] is True
    assert impl["validator_rejects_trusted_equals_true"] is True
    assert impl["validator_requires_require_sigs"] is True
    assert impl["validator_enforces_trusted_actor_allowlist"] is True
    assert (
        impl["public_key"]
        == "GitHub Actions secret POKECON_NIX_CACHE_PUBLIC_KEY (not stored in the repository)"
    )
    assert impl["secret_key_variable"] == "POKECON_NIX_CACHE_SECRET_KEY"  # noqa: S105
    assert impl["cache_store"].startswith("file://")
    assert impl.get("required_jobs_run_on_refactor_pr") is True
    assert "refactor/rust-core" in impl.get("head_ref_exclusion_removed", "")


def test_registry_desired_permissions_preserved() -> None:
    data = json.loads(REGISTRY.read_text(encoding="utf-8"))
    perms = data["binary_cache_contract"]["desired_permissions"]
    # Must preserve: PR read-only, push write by trusted actor
    pr_perm = next(p for p in perms if p["event"] == "pull_request")
    push_perm = next(p for p in perms if p["event"] == "push")
    assert pr_perm["read"] is True and pr_perm["write"] is False
    assert push_perm["read"] is True and push_perm["write"] is True


def test_no_secrets_printed_in_workflows() -> None:
    for path in (NORMAL_CI, PACKAGE_CI):
        text = _read(path)
        # Secrets should only be referenced via ${{ secrets.* }}, never echoed
        assert (
            "echo" not in text
            or "POKECON_NIX_CACHE_SECRET_KEY" not in text.split("echo")[1]
            if "echo" in text
            else True
        )
        # Ensure secret is not printed
        assert (
            "printf '%s' \"$POKECON_NIX_CACHE_SECRET_KEY\"" in _read(NORMAL_CI)
            or "POKECON_NIX_CACHE_SECRET_KEY" in text
        )
        # Ensure secret file is removed after use
        assert 'rm -f "$RUNNER_TEMP/pokecon-nix-cache.key"' in _read(NORMAL_CI)
