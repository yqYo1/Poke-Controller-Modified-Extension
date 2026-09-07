#!/usr/bin/env bash

set -euo pipefail

readonly default_timeout_seconds=1200
readonly poll_seconds=10
readonly settlement_seconds=120
readonly minimum_timeout_seconds=$((settlement_seconds + poll_seconds))
readonly failure_exit=1
readonly timeout_exit=124

usage() {
  printf '%s\n' \
    "usage: nix run .#ci-watch -- [branch] [timeout-seconds]" \
    "" \
    "defaults:" \
    "  branch: current Git branch" \
    "  timeout: ${default_timeout_seconds} seconds" \
    "  minimum timeout: ${minimum_timeout_seconds} seconds" \
    "" \
    "The watcher resolves the remote branch HEAD SHA at start (refs/remotes/origin/<branch>)" \
    "and enforces the two required branch-protection contexts for that exact SHA:" \
    "  - Normal CI Required" \
    "  - Package CI Required" \
    "Both contexts must be present for the target SHA, have status completed and" \
    "conclusion success. Missing, stale-SHA, in-progress/queued, skipped, neutral," \
    "cancelled, timed_out, action_required, or failure conclusions are not success." \
    "Unrelated optional checks are ignored once the two required contexts are" \
    "complete. The watcher polls every ${poll_seconds}s and succeeds only after the" \
    "required contexts have remained successful and unchanged for ${settlement_seconds}s." \
    "" \
    "exit statuses:" \
    "  0  both required contexts completed successfully and settled" \
    "  1  a required context completed with non-success conclusion" \
    "  2  usage or environment error" \
    "  ${timeout_exit}  timed out waiting for the required contexts" \
    "" \
    "polling and the settlement window are preserved; branch and timeout CLI are" \
    "preserved; tools are fixed Nix-provided (gh, git, jq, date, sleep, sort);" \
    "no secret output is emitted. Pagination and duplicate historical check-runs" \
    "are handled fail-closed (latest check-run by id for the target SHA wins)."
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac

if (($# > 2)); then
  usage >&2
  exit 2
fi

for command in date gh git jq sleep sort; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "required command is unavailable: ${command}" >&2
    exit 2
  fi
done

branch="${1:-}"
if [[ -z "${branch}" ]]; then
  branch="$(git branch --show-current)"
fi
readonly branch
readonly timeout_seconds="${2:-${default_timeout_seconds}}"

if [[ -z "${branch}" ]]; then
  echo "unable to determine the current branch" >&2
  exit 2
fi

if [[ ! "${timeout_seconds}" =~ ^[1-9][0-9]*$ ]]; then
  echo "timeout must be a positive number of seconds: ${timeout_seconds}" >&2
  exit 2
fi
if ((timeout_seconds < minimum_timeout_seconds)); then
  usage >&2
  echo "timeout must be at least ${minimum_timeout_seconds} seconds to cover one poll after the settlement window: ${timeout_seconds}" >&2
  exit 2
fi

readonly remote_ref="refs/remotes/origin/${branch}"
if ! sha="$(git rev-parse --verify "${remote_ref}^{commit}" 2>/dev/null)"; then
  echo "remote branch is unavailable: origin/${branch}" >&2
  exit 2
fi
readonly sha

started_at="$(date +%s)"
readonly started_at
last_change_at="${started_at}"
last_summary=""

echo "watching required checks for ${branch} at ${sha}: Normal CI Required, Package CI Required"

while true; do
  now="$(date +%s)"
  elapsed=$((now - started_at))
  if ((elapsed >= timeout_seconds)); then
    echo "timed out after ${timeout_seconds}s waiting for required checks Normal CI Required and Package CI Required at ${sha}" >&2
    echo "timeout and completed required-check failure use distinct exit statuses (${timeout_exit} vs ${failure_exit})" >&2
    exit "${timeout_exit}"
  fi

  if ! api_output="$(
    gh api --paginate --slurp "repos/{owner}/{repo}/commits/${sha}/check-runs?per_page=100" 2>&1
  )"; then
    echo "GitHub Checks query failed; retrying" >&2
    last_change_at="${now}"
    sleep "${poll_seconds}"
    continue
  fi

  if ! checks_tsv="$(
    jq -r --arg sha "${sha}" '
      [ .[] | .check_runs[] ] | map(select(.head_sha == $sha)) as $checks
      | ["Normal CI Required","Package CI Required"][] as $req
      | ($checks | map(select(.name == $req)) | sort_by(.id) | reverse | .[0] // null) as $latest
      | if $latest == null then
          "\($req)\tMISSING\t\t\t\t\t"
        else
          "\($latest.name)\t\($latest.status)\t\($latest.conclusion // "")\t\($latest.id)\t\($latest.html_url // "")\t\($latest.completed_at // "")\t\($latest.started_at // "")"
        end
    ' <<<"${api_output}" 2>&1
  )"; then
    echo "GitHub Checks response parse failed; retrying" >&2
    last_change_at="${now}"
    sleep "${poll_seconds}"
    continue
  fi

  # Build per-context summary and decide state.
  summary_lines=""
  failed=false
  pending=false

  while IFS=$'\t' read -r name status conclusion check_id html_url _completed_at _started_at; do
    # Empty line guard (jq always outputs two lines, but be defensive)
    if [[ -z "${name}" ]]; then
      continue
    fi
    if [[ "${status}" == "MISSING" ]]; then
      pending=true
      summary_lines+="${name}:missing"$'\n'
      continue
    fi
    if [[ "${status}" != "completed" ]]; then
      pending=true
      summary_lines+="${name}:${status}:${conclusion}:${check_id}"$'\n'
      continue
    fi
    if [[ "${conclusion}" != "success" ]]; then
      # Completed but not successful -> immediate fail-closed failure.
      if [[ -n "${html_url}" ]]; then
        echo "required check failed: ${name} (${conclusion}) ${html_url}" >&2
      else
        echo "required check failed: ${name} (${conclusion})" >&2
      fi
      failed=true
      summary_lines+="${name}:completed:${conclusion}:${check_id}"$'\n'
      continue
    fi
    summary_lines+="${name}:completed:success:${check_id}"$'\n'
  done <<<"${checks_tsv}"

  summary="$(sort <<<"${summary_lines}")"
  if [[ "${summary}" != "${last_summary}" ]]; then
    printf '%s\n' "${summary}"
    last_summary="${summary}"
    last_change_at="${now}"
  fi

  if [[ "${failed}" == true ]]; then
    exit "${failure_exit}"
  fi

  if [[ "${pending}" == true ]]; then
    sleep "${poll_seconds}"
    continue
  fi

  settled_for=$((now - last_change_at))
  if ((settled_for >= settlement_seconds)); then
    echo "all required checks completed successfully after the ${settlement_seconds}s settlement window"
    exit 0
  fi

  sleep "${poll_seconds}"
done
