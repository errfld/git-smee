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
## Required Implementation Workflow

Follow this workflow for every implementation task:

1. Determine the best issue to start with (priority, unblocking impact, and overall sequencing), then pick an **open GitHub issue**.
2. Mark the issue as `status:in_progress` and assign yourself.
3. Create a worktree and branch named `gh-<NUMBER>/<short-description>` (e.g., `gh-48/agents-workflow-github-only`).
4. Start implementation.
   - 4.1 Before making changes, retrieve existing knowledge:
     - `gh issue list --label memory --search "<domain or behavior>"`.
     - `gh issue list --label decision --search "<topic>"`.
     - `gh issue view <issue>` -- comments on blockers, parents, and linked follow-ups.
     - Read any prior task notes for linked issues to avoid duplicate rediscovery.
   - 4.2 Capture operational/process improvements and implementation guidance in GitHub issue comments on the active issue (not in `AGENTS.md`).
   - 4.3 Create new GitHub issues for discovered improvements/reworks with complete context for someone with no prior repository knowledge.
   - 4.4 Ensure behavior is correct, tests are sufficient, and code is production-ready.
   - 4.5 Review your own changes before publishing.
5. Push the branch and open a pull request.
6. Post a structured implementation note comment on the issue using `.github/templates/implementation-note.md` (or equivalent), including summary, validation, learnings, and follow-ups.
7. Wait for review and address PR comments.
8. Once the PR is merged by a maintainer or authorized reviewer, close the linked GitHub issue.

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

## Implementation Guidance

- Normalize user-provided config paths from `--config` and `GIT_SMEE_CONFIG` before use; on Unix this includes expanding leading `~` so runtime reads and installed hook scripts reference real absolute paths.

## Learnings

- Config extension checks should be ASCII case-insensitive so `.TOML`/mixed-case filenames work consistently across macOS and Windows defaults.
- Issue #51: expanding leading `~` during CLI config path resolution avoids writing literal-tilde paths into hook scripts and keeps install/run behavior consistent across interactive and non-interactive environments.
- Command redaction should use quote-aware tokenization and strict `KEY=value` detection so inline env secrets stay hidden even when values contain spaces.
- Keep CLI regression coverage for empty hook configs so `git smee install` continues surfacing a human-readable `No hooks present...` error instead of internal enum names.

