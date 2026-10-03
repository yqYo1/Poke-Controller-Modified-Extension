"""Regression: intentional second builds are exempt from duplicate reduction.

``package.yml`` verifies reproducibility by building each package twice from
identical inputs (``linux`` + ``linux_repro``, ``windows`` + ``windows_repro``)
and comparing the outputs in ``repro_check`` / ``windows_repro_check``.
AR-10.10-PACKAGE-REPRO keeps this item open until the pairs are documented as
intentional exceptions to Normal CI duplicate reduction and the inventory is
tested.

Contract under test (``docs/PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md`` is the
inventory, ``.github/workflows/package.yml`` is normative):

* every inventory pair refers to actual ``package.yml`` job names;
* the inventory records the reason (intentional, excluded from duplicate
  reduction) and the output comparison for each pair;
* the comparison jobs wire both the primary and the reproduction build via
  ``needs:``.

The test parses the workflow with the standard library only and fails closed:
an unparsable workflow, a renamed job, or an inventory that stops mentioning a
pair fails instead of silently passing. It never modifies the workflows.
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
PACKAGE_WORKFLOW = REPOSITORY / ".github/workflows/package.yml"
INVENTORY = REPOSITORY / "docs/PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md"

# (primary job, second-build job, comparison job).
PAIRS: tuple[tuple[str, str, str], ...] = (
    ("linux", "linux_repro", "repro_check"),
    ("windows", "windows_repro", "windows_repro_check"),
)

# Per-pair markers proving the inventory records the output comparison.
COMPARISON_MARKERS: dict[str, tuple[str, ...]] = {
    "repro_check": ("package-reproducibility-check",),
    "windows_repro_check": (
        "windows-payload-manifest.json",
        "windows-install-tree-manifest.json",
    ),
}

_JOB_SECTION_RE = re.compile(r"(?m)^jobs:\s*$")
_JOB_NAME_RE = re.compile(r"(?m)^  ([A-Za-z0-9_]+):\s*$")
_NEEDS_RE_TEMPLATE = r"(?m)^  {job}:\n(?:^.*\n)*?^    needs:\s*\[([^\]]*)\]"


def _workflow_job_names() -> set[str]:
    text = PACKAGE_WORKFLOW.read_text(encoding="utf-8")
    section = _JOB_SECTION_RE.search(text)
    assert section is not None, f"{PACKAGE_WORKFLOW}: cannot find jobs section"
    names = set(_JOB_NAME_RE.findall(text, section.end()))
    assert names, f"{PACKAGE_WORKFLOW}: no job names parsed"
    return names


def _workflow_needs(job: str) -> set[str]:
    text = PACKAGE_WORKFLOW.read_text(encoding="utf-8")
    match = re.search(_NEEDS_RE_TEMPLATE.format(job=re.escape(job)), text)
    assert match is not None, f"{PACKAGE_WORKFLOW}: cannot parse needs of {job}"
    return {
        part.strip().strip("'\"") for part in match.group(1).split(",") if part.strip()
    }


def _inventory_text() -> str:
    assert INVENTORY.is_file(), f"inventory document missing: {INVENTORY}"
    text = INVENTORY.read_text(encoding="utf-8")
    assert text.strip(), f"inventory document is empty: {INVENTORY}"
    return text


@pytest.mark.parametrize(
    ("primary", "repro", "check"), PAIRS, ids=[check for _, _, check in PAIRS]
)
def test_second_build_pair_refers_to_actual_package_jobs(
    primary: str, repro: str, check: str
) -> None:
    """Each inventory pair must name jobs existing in package.yml."""
    jobs = _workflow_job_names()
    assert primary in jobs, f"{PACKAGE_WORKFLOW}: primary job missing: {primary}"
    assert repro in jobs, f"{PACKAGE_WORKFLOW}: second-build job missing: {repro}"
    assert check in jobs, f"{PACKAGE_WORKFLOW}: comparison job missing: {check}"


@pytest.mark.parametrize(
    ("primary", "repro", "check"), PAIRS, ids=[check for _, _, check in PAIRS]
)
def test_inventory_documents_each_pair_job(
    primary: str, repro: str, check: str
) -> None:
    """The inventory must mention every job of the pair by exact name."""
    text = _inventory_text()
    for job in (primary, repro, check):
        assert f"`{job}`" in text, f"{INVENTORY}: pair job not documented: {job}"


@pytest.mark.parametrize(
    ("_primary", "_repro", "check"), PAIRS, ids=[check for _, _, check in PAIRS]
)
def test_inventory_records_reason_and_output_comparison(
    _primary: str, _repro: str, check: str
) -> None:
    """The inventory must record why the pair exists and what is compared."""
    text = _inventory_text()
    assert "重複削減対象外" in text, f"{INVENTORY}: exception reason not recorded"
    for marker in COMPARISON_MARKERS[check]:
        assert marker in text, f"{INVENTORY}: output comparison not recorded: {marker}"
    workflow_text = PACKAGE_WORKFLOW.read_text(encoding="utf-8")
    for marker in COMPARISON_MARKERS[check]:
        assert marker in workflow_text, (
            f"{PACKAGE_WORKFLOW}: comparison marker vanished: {marker}"
        )


@pytest.mark.parametrize(
    ("primary", "repro", "check"), PAIRS, ids=[check for _, _, check in PAIRS]
)
def test_comparison_job_wires_primary_and_reproduction_build(
    primary: str, repro: str, check: str
) -> None:
    """The comparison job must depend on both builds it compares."""
    needs = _workflow_needs(check)
    assert primary in needs, f"{PACKAGE_WORKFLOW}: {check} needs {primary}"
    assert repro in needs, f"{PACKAGE_WORKFLOW}: {check} needs {repro}"


def test_inventory_pins_same_version_probe_scope() -> None:
    """The inventory accepts the same-version probe without older-version scope."""
    text = _inventory_text()
    assert "same-version" in text, f"{INVENTORY}: same-version scope not recorded"
    assert "SkippedVerified" in text, f"{INVENTORY}: accepted probe not recorded"
