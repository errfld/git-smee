# Agent Instructions

This project uses **GitHub Issues + GitHub Projects** for task tracking. Use the GitHub CLI (`gh`) as the default interface.

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

## Comment Formatting (GitHub CLI)

Use real newlines for multi-line comments. Do **not** put `\n` inside normal quoted strings (for example, `"line1\nline2"`), because the shell passes it literally and GitHub renders `\n` as text. This applies to both `gh issue comment` and `gh pr comment`.

```bash
# Preferred: write multiline issue comments via stdin
gh issue comment <number> --body-file - <<'EOF'
Addressed all requested review updates in commit <sha>:
- item 1
- item 2

Both unresolved review threads are now marked resolved.
EOF

# Then close separately (or keep one-line close comments only)
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
