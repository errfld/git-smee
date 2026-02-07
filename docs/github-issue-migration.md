# Beads to GitHub Migration Plan

This document defines the phased migration from Beads (`bd`) to GitHub-native issue tracking.

## Goals

- Replace Beads as the source of truth for task tracking.
- Preserve task dependencies and hierarchy.
- Keep workflows agent-friendly and scriptable.

## Target System

- GitHub Issues for work items.
- GitHub sub-issues for hierarchy.
- GitHub issue dependencies (`blocked by` / `blocking`) for execution order.
- GitHub Projects (v2) for planning views and status tracking.

## Field and Label Model

Use labels as canonical metadata:

- Priority: `priority:P0`, `priority:P1`, `priority:P2`, `priority:P3`, `priority:P4`
- Type: `type:task`, `type:bug`, `type:feature`, `type:docs`, `type:question`
- Status: `status:todo`, `status:in_progress`, `status:blocked`, `status:done`
- Memory: `memory`, `decision`

## Migration Phases

1. Foundation
- Update agent instructions to `gh` workflows.
- Define labels and project views.
- Confirm repository permissions for issue dependency/sub-issue APIs.

2. Data Port
- Export Beads issue list and edges.
- Create GitHub issues first and save `beads_id -> github_issue_number` map.
- Apply hierarchy edges (parent/sub-issue) using mapped IDs.
- Apply dependency edges using mapped IDs.

3. Cutover
- Set Beads read-only for contributors/agents.
- Remove Beads commands from docs and scripts.
- Route all new tasks to GitHub Issues.

4. Stabilization
- Validate issue counts and edge counts between systems.
- Spot-check parent/sub-issue trees and blocked chains.
- Track migration defects as GitHub issues.

## Command Mapping

- `bd ready` -> GitHub project "Ready" view or `gh issue list --state open --search 'label:"status:todo" -label:"status:blocked"'`
- `bd show <id>` -> `gh issue view <number>`
- `bd update <id> --status in_progress` -> `gh issue edit <number> --add-label "status:in_progress" --remove-label "status:todo"`
- `bd close <id>` -> `gh issue close <number>`
- `bd dep add <a> <b>` -> dependency API (`a` blocked by `b`)

## API Operations

```bash
REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)

# a blocked by b
B_ID=$(gh api "repos/$REPO/issues/<b>" --jq .id)
printf '{"issue_id": %s}\n' "$B_ID" \
  | gh api -X POST "repos/$REPO/issues/<a>/dependencies/blocked_by" \
      -H "Accept: application/vnd.github+json" \
      -H "X-GitHub-Api-Version: 2022-11-28" \
      --input -

# c is child of p
C_ID=$(gh api "repos/$REPO/issues/<c>" --jq .id)
printf '{"sub_issue_id": %s}\n' "$C_ID" \
  | gh api -X POST "repos/$REPO/issues/<p>/sub_issues" \
      -H "Accept: application/vnd.github+json" \
      -H "X-GitHub-Api-Version: 2022-11-28" \
      --input -
```

## TUI

Recommended TUI: `gh-dash`

```bash
gh extension install dlvhdr/gh-dash
gh dash
```

Use `gh-dash` for viewing/triage and `gh`/`gh api` for automated writes.
