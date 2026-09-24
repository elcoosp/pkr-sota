#!/usr/bin/env bash
# Post a perf-diff table as a PR comment. Used by the branch-history
# fallback path (B11 Path B): after `diff-perf.sh` produces a Markdown
# table, this script posts it to the most recent open PR against main
# (or the PR number in $PR_NUMBER) via the GitHub CLI.
#
# Usage: ./ci/scripts/post-pr-comment.sh <diff.md> [pr-number]
# Requires: gh CLI authenticated (CI provides GH_TOKEN / GITHUB_TOKEN).
set -euo pipefail
DIFF="${1:-}"
PR="${2:-${PR_NUMBER:-}}"
if [ -z "$DIFF" ] || [ ! -s "$DIFF" ]; then
    echo "Usage: $0 <diff.md> [pr-number]" >&2
    exit 1
fi
if [ -z "$PR" ]; then
    PR=$(gh pr list --base main --state open --limit 1 --json number --jq '.[0].number')
fi
if [ -z "$PR" ] || [ "$PR" = "null" ]; then
    echo "No open PR against main; printing diff instead:"
    cat "$DIFF"
    exit 0
fi
BODY="$(cat "$DIFF")"
gh pr comment "$PR" --body "## Perf diff vs main
$BODY"
