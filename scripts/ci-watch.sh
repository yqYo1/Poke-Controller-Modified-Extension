#!/usr/bin/env bash
# CI Watch - GitHub Actions CI監視スクリプト
# Usage: ./scripts/ci-watch.sh [branch-name] [timeout-seconds]
#   branch-name: 監視対象ブランチ（デフォルト: 現在のブランチ）
#   timeout-seconds: タイムアウト秒数（デフォルト: 600）

set -euo pipefail

BRANCH="${1:-$(git branch --show-current)}"
TIMEOUT="${2:-600}"
POLL_INTERVAL=15

if [ -z "$BRANCH" ]; then
    echo "Error: Could not determine branch name" >&2
    exit 1
fi

echo "=== CI Watch: Monitoring branch '$BRANCH' ==="
echo "Timeout: ${TIMEOUT}s, Poll interval: ${POLL_INTERVAL}s"
echo ""

# Get the latest commit SHA on this branch
COMMIT_SHA=$(git rev-parse HEAD)
echo "Commit: ${COMMIT_SHA:0:8}"

# Wait for CI runs to appear
echo "Waiting for CI runs to start..."
for i in $(seq 1 30); do
    RUNS=$(gh run list --branch "$BRANCH" --limit 1 --json databaseId,status,conclusion,headSha 2>/dev/null || echo "[]")
    if echo "$RUNS" | grep -q "$COMMIT_SHA"; then
        break
    fi
    sleep 2
done

# Monitor CI progress
START_TIME=$(date +%s)
LAST_STATUS=""
FAILED_RUNS=""

while true; do
    ELAPSED=$(($(date +%s) - START_TIME))
    if [ "$ELAPSED" -gt "$TIMEOUT" ]; then
        echo ""
        echo "=== TIMEOUT (${TIMEOUT}s) ==="
        echo "Some jobs may still be running. Check manually:"
        echo "  gh run list --branch $BRANCH"
        exit 2
    fi

    # Get all runs for this commit
    RUNS_JSON=$(gh run list --branch "$BRANCH" --limit 10 --json databaseId,name,status,conclusion,headSha,startedAt 2>/dev/null || echo "[]")
    
    # Filter runs for current commit
    CURRENT_RUNS=$(echo "$RUNS_JSON" | jq --arg sha "$COMMIT_SHA" '[.[] | select(.headSha == $sha)]')
    
    if [ -z "$CURRENT_RUNS" ] || [ "$CURRENT_RUNS" = "[]" ]; then
        echo -n "."
        sleep "$POLL_INTERVAL"
        continue
    fi

    TOTAL=$(echo "$CURRENT_RUNS" | jq 'length')
    COMPLETED=$(echo "$CURRENT_RUNS" | jq '[.[] | select(.status == "completed")] | length')
    SUCCESS=$(echo "$CURRENT_RUNS" | jq '[.[] | select(.status == "completed" and .conclusion == "success")] | length')
    FAILED=$(echo "$CURRENT_RUNS" | jq '[.[] | select(.status == "completed" and .conclusion != "success")] | length')
    
    # Build status line
    STATUS_LINE="[$ELAPSED/${TIMEOUT}s] Jobs: $TOTAL | Completed: $COMPLETED | ✅ $SUCCESS | ❌ $FAILED"
    
    # Only print if status changed
    if [ "$STATUS_LINE" != "$LAST_STATUS" ]; then
        echo ""
        echo "$STATUS_LINE"
        LAST_STATUS="$STATUS_LINE"
        
        # Show failed jobs
        if [ "$FAILED" -gt 0 ]; then
            echo ""
            echo "Failed jobs:"
            echo "$CURRENT_RUNS" | jq -r '.[] | select(.status == "completed" and .conclusion != "success") | "  ❌ \(.name) (\(.conclusion))"'
        fi
    else
        echo -n "."
    fi

    # Check if all completed
    if [ "$COMPLETED" -eq "$TOTAL" ] && [ "$TOTAL" -gt 0 ]; then
        echo ""
        echo ""
        echo "=== CI Complete ==="
        if [ "$FAILED" -eq 0 ]; then
            echo "✅ ALL PASSED ($SUCCESS/$TOTAL jobs)"
            exit 0
        else
            echo "❌ FAILED ($FAILED/$TOTAL jobs failed)"
            echo ""
            echo "Failed jobs details:"
            echo "$CURRENT_RUNS" | jq -r '.[] | select(.status == "completed" and .conclusion != "success") | "  - \(.name): gh run view \(.databaseId) --log-failed"'
            exit 1
        fi
    fi

    sleep "$POLL_INTERVAL"
done
