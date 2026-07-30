#!/usr/bin/env bash

set -euo pipefail

readonly default_timeout_seconds=1200
readonly poll_seconds=10
readonly settlement_seconds=120
readonly minimum_timeout_seconds=$((settlement_seconds + poll_seconds))

usage() {
  printf '%s\n' \
    "usage: nix run .#ci-watch -- [branch] [timeout-seconds]" \
    "" \
    "defaults:" \
    "  branch: current Git branch" \
    "  timeout: ${default_timeout_seconds} seconds" \
    "  minimum timeout: ${minimum_timeout_seconds} seconds" \
    "" \
    "The watcher follows workflow runs discovered for the remote branch HEAD." \
    "It succeeds after every discovered run completes successfully and the" \
    "discovered run/status set remains unchanged for ${settlement_seconds} seconds." \
    "Until CI has one aggregate workflow, this does not guarantee that a later" \
    "workflow run will not appear after the settlement window."
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

for command in date gh git sleep sort; do
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
seen_runs=false

echo "watching GitHub Actions for ${branch} at ${sha}"

while true; do
  now="$(date +%s)"
  elapsed=$((now - started_at))
  if ((elapsed >= timeout_seconds)); then
    echo "timed out after ${timeout_seconds}s waiting for GitHub Actions" >&2
    exit 1
  fi

  if ! run_lines="$(
    gh run list \
      --branch "${branch}" \
      --commit "${sha}" \
      --limit 100 \
      --json databaseId,workflowName,status,conclusion,url \
      --jq '.[] | [.databaseId, .workflowName, .status, (.conclusion // ""), .url] | @tsv'
  )"; then
    echo "GitHub Actions query failed; retrying" >&2
    last_change_at="${now}"
    sleep "${poll_seconds}"
    continue
  fi

  if [[ -z "${run_lines}" ]]; then
    last_change_at="${now}"
    sleep "${poll_seconds}"
    continue
  fi

  seen_runs=true
  summary="$(sort <<<"${run_lines}")"
  if [[ "${summary}" != "${last_summary}" ]]; then
    printf '%s\n' "${summary}"
    last_summary="${summary}"
    last_change_at="${now}"
  fi

  all_complete=true
  failed=false
  while IFS=$'\t' read -r _ workflow status conclusion url; do
    if [[ "${status}" != "completed" ]]; then
      all_complete=false
      continue
    fi

    case "${conclusion}" in
      success | neutral | skipped) ;;
      *)
        echo "workflow failed: ${workflow} (${conclusion}) ${url}" >&2
        failed=true
        ;;
    esac
  done <<<"${run_lines}"

  if [[ "${failed}" == true ]]; then
    exit 1
  fi

  settled_for=$((now - last_change_at))
  if [[ "${seen_runs}" == true && "${all_complete}" == true && settled_for -ge settlement_seconds ]]; then
    echo "all discovered GitHub Actions runs completed successfully after the ${settlement_seconds}s settlement window"
    exit 0
  fi

  sleep "${poll_seconds}"
done
