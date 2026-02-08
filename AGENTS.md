# Agent Instructions

This project uses **GitHub Issues + GitHub Projects** for task tracking. Use the GitHub CLI (`gh`) as the default interface.
GitHub Issues are the **only** source of truth in this repository. Do **not** use Beads/`bd`.

## Quick Reference

```bash
# Find work
gh issue list --state open --limit 100

# Inspect an issue
gh issue view <number>

# Claim work
gh issue edit <number> --add-assignee @me --add-label status:in_progress

# Create follow-up work
gh issue create --title "..." --body-file <file> --label type:task --label status:todo

# Complete work
gh issue close <number> --comment "Completed in <commit-or-pr>"
```

## Dependencies and Hierarchy

Use native GitHub relationships (not markdown-only links):

```bash
REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)

# Add dependency: issue <blocked> is blocked by issue <blocker>
BLOCKER_ID=$(gh api "repos/$REPO/issues/<blocker>" --jq .id)
printf '{"issue_id": %s}\n' "$BLOCKER_ID" \
  | gh api -X POST "repos/$REPO/issues/<blocked>/dependencies/blocked_by" --input -

# Add hierarchy: issue <child> becomes a sub-issue of issue <parent>
CHILD_ID=$(gh api "repos/$REPO/issues/<child>" --jq .id)
printf '{"sub_issue_id": %s}\n' "$CHILD_ID" \
  | gh api -X POST "repos/$REPO/issues/<parent>/sub_issues" --input -
```

## Landing the Plane (Session Completion)

**When ending a work session**, complete ALL steps below. Work is NOT complete until `git push` succeeds.

1. **File issues for remaining work** - create follow-up GitHub issues for any unfinished work.
2. **Run quality gates** (if code changed) - tests, linters, builds.
3. **Update issue state** - move active issue(s) to correct status labels and close completed issue(s).
4. **Push to remote** - mandatory:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - clear stashes, prune stale branches.
6. **Verify** - all intended changes are committed and pushed.
7. **Hand off** - leave clear context in issue comments.

## Workflow Pattern

1. **Start**: Find unblocked work in GitHub Project views or issue search.
2. **Claim**: Assign yourself and add `status:in_progress`.
3. **Work**: Implement the task and keep issue comments updated.
4. **Complete**: Close the issue with commit/PR reference.
5. **Sync**: Ensure branch is rebased and pushed.

## Required Implementation Workflow

Follow this workflow for every implementation task:

1. Determine the best issue to start with (priority, unblocking impact, and overall sequencing), then pick an **open GitHub issue**.
2. Mark the issue as `status:in_progress` and assign yourself.
3. Create a worktree and branch named `gh-<NUMBER>/<short-description>`.
4. Start implementation.
   - 4.1 Note operational/process improvements and implementation guidance in `AGENTS.md`.
   - 4.2 Create new GitHub issues for discovered improvements/reworks with complete context for someone with no prior repository knowledge.
   - 4.3 Ensure behavior is correct, tests are sufficient, and code is production-ready.
   - 4.4 Review your own changes before publishing.
5. Push the branch and open a pull request.
6. Wait for review and address PR comments.
7. User merges the PR; then close the linked GitHub issue.

## Key Conventions

- **Dependencies**: Use issue dependency APIs (`blocked by` / `blocking`) for scheduling truth.
- **Hierarchy**: Use parent/sub-issue relationships for epics and decomposed work.
- **Priority**: Use labels `priority:P0` ... `priority:P4`.
- **Types**: Use labels `type:task`, `type:bug`, `type:feature`, `type:docs`, `type:question`.
- **Status**: Use labels `status:todo`, `status:in_progress`, `status:blocked`, `status:done`.
- **Agent memory**: Store durable decisions/context in issues labeled `memory` and/or `decision`; keep one memory thread per domain and append via sub-issues.

## TUI Recommendation

Use **gh-dash** for keyboard-driven issue/PR triage and project visibility:

```bash
gh extension install dlvhdr/gh-dash
gh dash
```

Prefer `gh` / `gh api` for deterministic write operations by agents.
