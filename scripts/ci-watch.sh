#!/usr/bin/env bash

set -euo pipefail

readonly branch="${1:-$(git branch --show-current)}"
readonly timeout_seconds="${2:-600}"
readonly poll_seconds=10
readonly stable_seconds=30

if [[ -z "${branch}" ]]; then
  echo "unable to determine the current branch" >&2
  exit 2
fi

if [[ ! "${timeout_seconds}" =~ ^[1-9][0-9]*$ ]]; then
  echo "timeout must be a positive number of seconds: ${timeout_seconds}" >&2
  exit 2
fi

for command in date gh git sort; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "required command is unavailable: ${command}" >&2
    exit 2
  fi
done

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
    sleep "${poll_seconds}"
    continue
  fi

  if [[ -z "${run_lines}" ]]; then
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

  stable_for=$((now - last_change_at))
  if [[ "${seen_runs}" == true && "${all_complete}" == true && stable_for -ge stable_seconds ]]; then
    echo "all discovered GitHub Actions runs completed successfully"
    exit 0
  fi

  sleep "${poll_seconds}"
done
