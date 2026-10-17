"""Regression: direct-push inspection and PR-head dedup trade-off (AR-10.10-05).

History: pushing head commit ``82b973e`` to ``refactor/rust-core`` while its
pull request against ``main`` was open produced both a ``push`` and
``pull_request`` workflow run. The first mitigation kept ``on.push`` limited
to default branches, which left PR-less pushes to the integration branch
uninspected.

Owner decision (2026-09-28, see PLAN.md AR-10.10-05): direct pushes to the
integration branch ``refactor/rust-core`` are allowed and must be inspected
via the ``push`` event. The same-head duplicate run that occurs while a pull
request is open is accepted best-effort: static ``on:`` filters cannot
distinguish cross-event duplicates, and GitHub uses the pushed tip for
``push.GITHUB_SHA`` but a synthetic merge commit for
``pull_request.GITHUB_SHA``.

Trigger contract under test (``.github/workflows/normal-ci.yml`` and
``.github/workflows/package.yml``):

* direct commits to a default branch (``main``/``master``) are checked once
  via ``push``;
* direct commits to the integration branch (``refactor/rust-core``) are
  checked via ``push`` even when no pull request exists;
* feature commits are checked via ``pull_request`` whose base may be a
  default or the integration branch;
* a push to the integration branch while its PR is open fires both events for
  one head commit; this duplicate is accepted best-effort.

The test parses the workflow ``on:`` trigger block with the standard library
only and simulates GitHub branch-filter matching for an event/branch matrix.
It fails closed: unparsable trigger blocks or renamed aggregate gates fail
instead of silently passing.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
WORKFLOWS = {
    REPOSITORY / ".github/workflows/normal-ci.yml": "Normal CI Required",
    REPOSITORY / ".github/workflows/package.yml": "Package CI Required",
}

FEATURE_BRANCH = "refactor/rust-core"
DEFAULT_BRANCHES = ("main", "master")
PUSH_BRANCHES = frozenset((*DEFAULT_BRANCHES, FEATURE_BRANCH))

_PUSH_RE = re.compile(
    r"(?m)^on:\s*\n(?:^[ ]+.*\n)*?^[ ]+push:\s*\n^[ ]+branches:\s*\[([^\]]*)\]",
)
_PR_RE = re.compile(
    r"(?m)^on:\s*\n(?:^[ ]+.*\n)*?^[ ]+pull_request:\s*\n^[ ]+branches:\s*\[([^\]]*)\]",
)
_REQUIRED_NAME_RE = re.compile(
    r"(?m)^  required:\s*\n(?:^[ ]+.*\n)*?^    name:\s*(.+?)\s*$"
)


def _branches(raw: str) -> frozenset[str]:
    return frozenset(
        part.strip().strip("'\"") for part in raw.split(",") if part.strip()
    )


@dataclass(frozen=True, slots=True)
class Trigger:
    push_branches: frozenset[str]
    pr_bases: frozenset[str]

    def runs_for(self, *, pushed_branch: str, pr_base: str | None) -> tuple[str, ...]:
        """Event names GitHub would fire for one pushed SHA.

        ``pr_base`` is the base of the open PR whose head contains the SHA, or
        ``None`` when the push has no associated pull request. When both
        events fire for one head commit the duplicate is accepted
        best-effort (AR-10.10-05).
        """
        runs: list[str] = []
        if pushed_branch in self.push_branches:
            runs.append("push")
        if pr_base is not None and pr_base in self.pr_bases:
            runs.append("pull_request")
        return tuple(runs)


def _load_trigger(path: Path) -> Trigger:
    text = path.read_text(encoding="utf-8")
    push_match = _PUSH_RE.search(text)
    pr_match = _PR_RE.search(text)
    assert push_match is not None, f"{path}: cannot parse on.push.branches"
    assert pr_match is not None, f"{path}: cannot parse on.pull_request.branches"
    push_branches = _branches(push_match.group(1))
    pr_bases = _branches(pr_match.group(1))
    assert push_branches, f"{path}: on.push.branches is empty"
    assert pr_bases, f"{path}: on.pull_request.branches is empty"
    return Trigger(push_branches=push_branches, pr_bases=pr_bases)


@pytest.fixture(params=sorted(str(path) for path in WORKFLOWS))
def workflow(request: pytest.FixtureRequest) -> tuple[Path, Trigger]:
    path = Path(str(request.param))
    return path, _load_trigger(path)


def test_direct_integration_branch_push_without_pr_is_checked_via_push(
    workflow: tuple[Path, Trigger],
) -> None:
    """AR-10.10-05: PR-less integration-branch pushes must trigger CI."""
    _, trigger = workflow
    assert trigger.runs_for(pushed_branch=FEATURE_BRANCH, pr_base=None) == ("push",)


def test_feature_branch_push_with_open_pr_is_accepted_duplicate(
    workflow: tuple[Path, Trigger],
) -> None:
    """Best-effort trade-off: both events fire for one head commit."""
    _, trigger = workflow
    assert trigger.runs_for(pushed_branch=FEATURE_BRANCH, pr_base="main") == (
        "push",
        "pull_request",
    )


def test_direct_default_branch_push_runs_once_via_push(
    workflow: tuple[Path, Trigger],
) -> None:
    _, trigger = workflow
    for branch in DEFAULT_BRANCHES:
        assert trigger.runs_for(pushed_branch=branch, pr_base=None) == ("push",)


def test_stacked_pr_targeting_integration_branch_is_checked(
    workflow: tuple[Path, Trigger],
) -> None:
    _, trigger = workflow
    assert trigger.runs_for(
        pushed_branch="feature/something", pr_base=FEATURE_BRANCH
    ) == ("pull_request",)


def test_push_covers_default_and_integration_branches(
    workflow: tuple[Path, Trigger],
) -> None:
    """Push inspects default branches and the integration branch exactly."""
    path, trigger = workflow
    assert trigger.push_branches == PUSH_BRANCHES, (
        f"{path}: on.push.branches must be exactly {sorted(PUSH_BRANCHES)}, "
        f"got {sorted(trigger.push_branches)}"
    )


def test_pull_request_still_covers_default_and_integration_bases(
    workflow: tuple[Path, Trigger],
) -> None:
    _, trigger = workflow
    assert {"main", FEATURE_BRANCH} <= trigger.pr_bases


def test_required_aggregate_check_names_unchanged() -> None:
    for path, expected in WORKFLOWS.items():
        text = path.read_text(encoding="utf-8")
        match = _REQUIRED_NAME_RE.search(text)
        assert match is not None, f"{path}: cannot find required job name"
        assert match.group(1) == expected, (
            f"{path}: required check renamed to {match.group(1)!r}"
        )


def test_normal_and_package_ci_triggers_match() -> None:
    triggers = {path: _load_trigger(path) for path in WORKFLOWS}
    by_name = list(triggers.values())
    assert by_name[0] == by_name[1], (
        "Normal CI and Package CI trigger matrices diverged: "
        f"{[(str(p), t) for p, t in triggers.items()]}"
    )
