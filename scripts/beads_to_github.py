#!/usr/bin/env python3
"""Migrate Beads issues into GitHub issues.

This script performs migration in two phases:
1. Create GitHub issues from a Beads JSONL export.
2. Apply optional dependency and hierarchy links from a JSON file.

By default it runs in dry-run mode. Use --apply to execute write operations.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

API_HEADERS = [
    "Accept: application/vnd.github+json",
    "X-GitHub-Api-Version: 2022-11-28",
]


def run_cmd(cmd: list[str], *, stdin_text: str | None = None) -> str:
    proc = subprocess.run(
        cmd,
        input=stdin_text,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        stderr = proc.stderr.strip() or "(no stderr)"
        stdout = proc.stdout.strip()
        details = f"\nstdout:\n{stdout}" if stdout else ""
        raise RuntimeError(f"Command failed ({proc.returncode}): {' '.join(cmd)}\nstderr:\n{stderr}{details}")
    return proc.stdout.strip()


def detect_repo() -> str:
    return run_cmd(["gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    issues: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as f:
        for lineno, line in enumerate(f, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"Invalid JSON in {path}:{lineno}: {exc}") from exc
            if not isinstance(obj, dict):
                raise ValueError(f"Expected object in {path}:{lineno}, got {type(obj).__name__}")
            issues.append(obj)
    return issues


def load_mapping(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"repo": None, "issues": {}}
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"Mapping file {path} must be a JSON object")
    data.setdefault("repo", None)
    data.setdefault("issues", {})
    if not isinstance(data["issues"], dict):
        raise ValueError(f"Mapping file {path} field 'issues' must be an object")
    return data


def save_mapping(path: Path, mapping: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(mapping, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_links(path: Path | None) -> dict[str, list[dict[str, Any]]]:
    if path is None:
        return {"dependencies": [], "hierarchy": []}
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"Links file {path} must be a JSON object")
    deps = data.get("dependencies", [])
    hierarchy = data.get("hierarchy", [])
    if not isinstance(deps, list) or not isinstance(hierarchy, list):
        raise ValueError(f"Links file {path} fields 'dependencies' and 'hierarchy' must be arrays")
    return {"dependencies": deps, "hierarchy": hierarchy}


def issue_type_label(raw: Any) -> str:
    mapping = {
        "task": "type:task",
        "bug": "type:bug",
        "feature": "type:feature",
        "docs": "type:docs",
        "question": "type:question",
    }
    key = str(raw or "task").strip().lower()
    return mapping.get(key, "type:task")


def priority_label(raw: Any) -> str:
    try:
        p = int(raw)
    except (TypeError, ValueError):
        p = 2
    if p < 0:
        p = 0
    if p > 4:
        p = 4
    return f"priority:P{p}"


def status_label(raw: Any) -> str:
    s = str(raw or "open").strip().lower()
    if s in {"closed", "done", "resolved"}:
        return "status:done"
    if s in {"blocked"}:
        return "status:blocked"
    if s in {"in_progress", "started", "active"}:
        return "status:in_progress"
    return "status:todo"


def build_labels(issue: dict[str, Any]) -> list[str]:
    labels = {
        issue_type_label(issue.get("issue_type")),
        priority_label(issue.get("priority")),
        status_label(issue.get("status")),
    }
    return sorted(labels)


def clean_text(value: Any) -> str:
    text = str(value or "").strip()
    return text


def build_issue_body(issue: dict[str, Any]) -> str:
    issue_id = clean_text(issue.get("id"))
    created_at = clean_text(issue.get("created_at"))
    updated_at = clean_text(issue.get("updated_at"))
    owner = clean_text(issue.get("owner"))
    created_by = clean_text(issue.get("created_by"))
    status = clean_text(issue.get("status"))

    parts: list[str] = []
    description = clean_text(issue.get("description"))
    if description:
        parts.append("## Description\n" + description)

    acceptance = clean_text(issue.get("acceptance_criteria"))
    if acceptance:
        parts.append("## Acceptance Criteria\n" + acceptance)

    notes = clean_text(issue.get("notes"))
    if notes:
        parts.append("## Notes\n" + notes)

    metadata_lines = [
        f"- Source: beads",
        f"- Beads ID: {issue_id or '(missing)'}",
        f"- Original status: {status or '(missing)'}",
    ]
    if created_at:
        metadata_lines.append(f"- Created at: {created_at}")
    if updated_at:
        metadata_lines.append(f"- Updated at: {updated_at}")
    if owner:
        metadata_lines.append(f"- Owner: {owner}")
    if created_by:
        metadata_lines.append(f"- Created by: {created_by}")

    parts.append("## Migration Metadata\n" + "\n".join(metadata_lines))
    return "\n\n".join(parts).strip() + "\n"


def parse_issue_number_from_url(url: str) -> int:
    match = re.search(r"/(\d+)$", url.strip())
    if not match:
        raise ValueError(f"Could not parse issue number from URL: {url}")
    return int(match.group(1))


def create_github_issue(repo: str, issue: dict[str, Any], *, dry_run: bool) -> int | None:
    title = clean_text(issue.get("title")) or f"migrated: {clean_text(issue.get('id'))}"
    labels = build_labels(issue)

    if dry_run:
        print(f"[DRY RUN] create issue: title={title!r}, labels={labels}")
        return None

    body = build_issue_body(issue)
    with tempfile.NamedTemporaryFile("w", suffix=".md", delete=True, encoding="utf-8") as tmp:
        tmp.write(body)
        tmp.flush()
        cmd = [
            "gh",
            "issue",
            "create",
            "--repo",
            repo,
            "--title",
            title,
            "--body-file",
            tmp.name,
        ]
        for label in labels:
            cmd.extend(["--label", label])
        url = run_cmd(cmd)

    number = parse_issue_number_from_url(url)
    print(f"Created #{number} for beads issue {issue.get('id')}")
    return number


def close_github_issue(repo: str, number: int, issue: dict[str, Any], *, dry_run: bool) -> None:
    reason = clean_text(issue.get("close_reason"))
    beads_id = clean_text(issue.get("id"))
    comment = f"Migrated from beads issue {beads_id}."
    if reason:
        comment += f" Close reason: {reason}."

    if dry_run:
        print(f"[DRY RUN] close issue #{number}")
        return

    run_cmd([
        "gh",
        "issue",
        "close",
        str(number),
        "--repo",
        repo,
        "--comment",
        comment,
    ])
    print(f"Closed #{number} (source was closed)")


def get_issue_numeric_id(repo: str, number: int) -> int:
    out = run_cmd(["gh", "api", f"repos/{repo}/issues/{number}", "--jq", ".id"])
    return int(out)


def get_blocked_by_numbers(repo: str, blocked_number: int) -> set[int]:
    out = run_cmd(
        [
            "gh",
            "api",
            f"repos/{repo}/issues/{blocked_number}/dependencies/blocked_by",
            "--jq",
            "map(.number) | .[]",
        ]
    )
    if not out:
        return set()
    return {int(x) for x in out.splitlines() if x.strip()}


def get_sub_issue_numbers(repo: str, parent_number: int) -> set[int]:
    out = run_cmd(
        [
            "gh",
            "api",
            f"repos/{repo}/issues/{parent_number}/sub_issues",
            "--jq",
            "map(.number) | .[]",
        ]
    )
    if not out:
        return set()
    return {int(x) for x in out.splitlines() if x.strip()}


def add_dependency(repo: str, blocked_number: int, blocker_number: int, *, dry_run: bool) -> None:
    if dry_run:
        print(f"[DRY RUN] add dependency: #{blocked_number} blocked by #{blocker_number}")
        return

    existing = get_blocked_by_numbers(repo, blocked_number)
    if blocker_number in existing:
        print(f"Dependency already exists: #{blocked_number} blocked by #{blocker_number}")
        return

    blocker_id = get_issue_numeric_id(repo, blocker_number)
    payload = json.dumps({"issue_id": blocker_id})
    cmd = [
        "gh",
        "api",
        "-X",
        "POST",
        f"repos/{repo}/issues/{blocked_number}/dependencies/blocked_by",
    ]
    for header in API_HEADERS:
        cmd.extend(["-H", header])
    cmd.extend(["--input", "-"])
    run_cmd(cmd, stdin_text=payload)
    print(f"Added dependency: #{blocked_number} blocked by #{blocker_number}")


def add_sub_issue(repo: str, parent_number: int, child_number: int, *, dry_run: bool) -> None:
    if dry_run:
        print(f"[DRY RUN] add hierarchy: #{child_number} under #{parent_number}")
        return

    existing = get_sub_issue_numbers(repo, parent_number)
    if child_number in existing:
        print(f"Sub-issue already exists: #{child_number} under #{parent_number}")
        return

    child_id = get_issue_numeric_id(repo, child_number)
    payload = json.dumps({"sub_issue_id": child_id})
    cmd = [
        "gh",
        "api",
        "-X",
        "POST",
        f"repos/{repo}/issues/{parent_number}/sub_issues",
    ]
    for header in API_HEADERS:
        cmd.extend(["-H", header])
    cmd.extend(["--input", "-"])
    run_cmd(cmd, stdin_text=payload)
    print(f"Added hierarchy: #{child_number} under #{parent_number}")


def resolve_issue_ref(ref: Any, mapping: dict[str, int]) -> int | None:
    if isinstance(ref, int):
        return ref
    text = str(ref or "").strip()
    if not text:
        return None
    if text.startswith("#") and text[1:].isdigit():
        return int(text[1:])
    if text.isdigit():
        return int(text)
    return mapping.get(text)


def migrate_issues(
    *,
    repo: str,
    issues: list[dict[str, Any]],
    mapping_path: Path,
    mapping_data: dict[str, Any],
    dry_run: bool,
    open_only: bool,
    limit: int | None,
) -> tuple[dict[str, int], int, int]:
    issue_map: dict[str, int] = {
        key: int(value) for key, value in mapping_data.get("issues", {}).items()
    }

    selected = issues
    if open_only:
        selected = [i for i in selected if str(i.get("status", "")).lower() == "open"]
    if limit is not None:
        selected = selected[:limit]

    created_count = 0
    planned_count = 0

    for issue in selected:
        beads_id = clean_text(issue.get("id"))
        if not beads_id:
            print("Skipping issue without id")
            continue

        if beads_id in issue_map:
            print(f"Skipping {beads_id}: already mapped to #{issue_map[beads_id]}")
            continue

        planned_count += 1
        number = create_github_issue(repo, issue, dry_run=dry_run)
        if number is None:
            continue

        issue_map[beads_id] = number
        created_count += 1

        mapping_data["repo"] = repo
        mapping_data["issues"] = issue_map
        save_mapping(mapping_path, mapping_data)

        if str(issue.get("status", "")).strip().lower() == "closed":
            close_github_issue(repo, number, issue, dry_run=dry_run)

    return issue_map, created_count, planned_count


def apply_links(
    *,
    repo: str,
    links: dict[str, list[dict[str, Any]]],
    issue_map: dict[str, int],
    dry_run: bool,
) -> tuple[int, int]:
    dep_count = 0
    hier_count = 0

    for dep in links.get("dependencies", []):
        if not isinstance(dep, dict):
            print(f"Skipping malformed dependency entry: {dep!r}")
            continue
        blocked_num = resolve_issue_ref(dep.get("blocked"), issue_map)
        blocker_num = resolve_issue_ref(dep.get("blocker"), issue_map)
        if blocked_num is None or blocker_num is None:
            print(f"Skipping dependency with unmapped refs: {dep}")
            continue
        add_dependency(repo, blocked_num, blocker_num, dry_run=dry_run)
        dep_count += 1

    for rel in links.get("hierarchy", []):
        if not isinstance(rel, dict):
            print(f"Skipping malformed hierarchy entry: {rel!r}")
            continue
        parent_num = resolve_issue_ref(rel.get("parent"), issue_map)
        child_num = resolve_issue_ref(rel.get("child"), issue_map)
        if parent_num is None or child_num is None:
            print(f"Skipping hierarchy with unmapped refs: {rel}")
            continue
        add_sub_issue(repo, parent_num, child_num, dry_run=dry_run)
        hier_count += 1

    return dep_count, hier_count


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--issues", type=Path, default=Path(".beads/issues.jsonl"), help="Path to Beads issues JSONL")
    parser.add_argument(
        "--mapping",
        type=Path,
        default=Path(".migration/beads_to_github_mapping.json"),
        help="Path to persistent beads->github issue mapping",
    )
    parser.add_argument(
        "--links",
        type=Path,
        default=None,
        help="Optional JSON file with dependency/hierarchy links",
    )
    parser.add_argument("--repo", default=None, help="GitHub repo in OWNER/REPO form")
    parser.add_argument("--open-only", action="store_true", help="Migrate only issues with status=open")
    parser.add_argument("--limit", type=int, default=None, help="Migrate at most N issues")
    parser.add_argument("--apply", action="store_true", help="Execute writes. Without this flag, script is dry-run")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    dry_run = not args.apply

    if not args.issues.exists():
        print(f"Issues file not found: {args.issues}", file=sys.stderr)
        return 1

    repo = args.repo or detect_repo()
    issues = load_jsonl(args.issues)
    mapping_data = load_mapping(args.mapping)

    mapped_repo = mapping_data.get("repo")
    if mapped_repo and mapped_repo != repo:
        print(
            f"Refusing to continue: mapping file is bound to repo {mapped_repo}, current repo is {repo}",
            file=sys.stderr,
        )
        return 1

    links = load_links(args.links)

    mode = "APPLY" if args.apply else "DRY RUN"
    print(f"Mode: {mode}")
    print(f"Repo: {repo}")
    print(f"Issues input: {args.issues}")
    print(f"Mapping file: {args.mapping}")
    if args.links:
        print(f"Links file: {args.links}")

    issue_map, created_count, planned_count = migrate_issues(
        repo=repo,
        issues=issues,
        mapping_path=args.mapping,
        mapping_data=mapping_data,
        dry_run=dry_run,
        open_only=args.open_only,
        limit=args.limit,
    )

    dep_count, hier_count = apply_links(repo=repo, links=links, issue_map=issue_map, dry_run=dry_run)

    print("Summary:")
    print(f"- Issues planned: {planned_count}")
    print(f"- Issues created: {created_count}")
    print(f"- Dependency links processed: {dep_count}")
    print(f"- Hierarchy links processed: {hier_count}")

    if dry_run:
        print("Dry-run complete. Re-run with --apply to execute migration.")
    else:
        print("Migration apply complete.")

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
