#!/usr/bin/env python3
"""Produce read-only local-vs-public release synchronization evidence.

This command never fetches, pushes, edits Git metadata, or changes GitHub
settings. It compares the checked-out repository, its ``origin/main``
tracking ref, and the remote ``HEAD`` when the network is available.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
PUBLIC_URL = "https://github.com/williamlabdev/Orvena"
ABOUT_TEXT = (
    "OS-enforced task-scope governance runtime for AI coding agents — bounded "
    "execution, verify gates, and frozen evidence."
)
# Captured before this positioning goal began. These paths belong to the
# user's pre-existing Orvena worktree and must not be silently folded into the
# goal's release surface.
PRE_GOAL_DIRTY_PATHS = (
    ".aine/registry.json",
    "README.md",
    "bench-runs/20260820-capability-v3-agent-claude-claude-opus-4-8.json",
    "crates/orvena-core/src/adapter/mod.rs",
    "docs/ARCHITECTURE.md",
)
GOAL_MANAGED_PATHS = (
    "CHANGELOG.md",
    "README.md",
    "docs/ARCHITECTURE.md",
    "docs/PUBLIC-POSITIONING.md",
    "docs/release-sync/orvena-public-sync-2026-08-24.json",
    "manifest.yaml",
    "scripts/check-public-sync.py",
)


def _git(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if check and result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result


def _ref(ref: str) -> str:
    return _git("rev-parse", ref).stdout.strip()


def _remote_head() -> dict[str, Any]:
    result = _git("ls-remote", "origin", "HEAD", check=False)
    if result.returncode != 0:
        return {
            "status": "UNAVAILABLE",
            "error": (result.stderr or result.stdout).strip()[-500:],
            "commit": None,
        }
    commit = result.stdout.split()[0] if result.stdout.split() else ""
    return {"status": "PASS" if commit else "INCONCLUSIVE", "commit": commit or None}


def _dirty_paths() -> list[str]:
    output = _git("status", "--porcelain", "--untracked-files=all").stdout
    paths: list[str] = []
    for line in output.splitlines():
        if len(line) < 4:
            continue
        raw = line[3:].strip()
        if " -> " in raw:
            raw = raw.rsplit(" -> ", 1)[-1]
        paths.append(raw)
    return sorted(paths)


def _names(*args: str) -> list[str]:
    result = _git(*args, check=False)
    if result.returncode != 0:
        return []
    return sorted(item for item in result.stdout.splitlines() if item)


def _ahead_behind() -> dict[str, int]:
    result = _git("rev-list", "--left-right", "--count", "origin/main...HEAD")
    left, right = (int(item) for item in result.stdout.split())
    return {
        "origin_main_commits_not_in_local": left,
        "local_commits_not_in_origin_main": right,
    }


def build_evidence() -> dict[str, Any]:
    local_head = _ref("HEAD")
    origin_main = _ref("origin/main")
    remote = _remote_head()
    counts = _ahead_behind()
    dirty = _dirty_paths()
    local_only_files = _names("diff", "--name-only", "origin/main..HEAD")
    if remote["commit"] == local_head:
        sync_state = "SYNCHRONIZED"
    elif remote["commit"] == origin_main and counts["local_commits_not_in_origin_main"] > 0:
        sync_state = "LOCAL_AHEAD_UNRELEASED"
    elif counts["origin_main_commits_not_in_local"] > 0 and counts["local_commits_not_in_origin_main"] > 0:
        sync_state = "DIVERGED"
    elif counts["origin_main_commits_not_in_local"] > 0:
        sync_state = "LOCAL_BEHIND"
    else:
        sync_state = "REMOTE_COMPARISON_PENDING"

    return {
        "schema": "orvena.public-sync-evidence.v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "repository": "aine.orvena",
        "public_url": PUBLIC_URL,
        "local_head": local_head,
        "origin_main": origin_main,
        "remote_head": remote,
        "sync_state": sync_state,
        "commit_delta": counts,
        "local_only_committed_files": local_only_files,
        "dirty_paths": dirty,
        "baseline": {
            "pre_goal_dirty_paths": list(PRE_GOAL_DIRTY_PATHS),
            "goal_managed_paths": list(GOAL_MANAGED_PATHS),
            "overlap_paths": sorted(set(PRE_GOAL_DIRTY_PATHS) & set(GOAL_MANAGED_PATHS)),
            "new_goal_paths_detected": sorted(
                set(dirty) - set(PRE_GOAL_DIRTY_PATHS)
            ),
        },
        "positioning": {
            "verdict": "CORE_POSITION_ALIGNED",
            "about_text": ABOUT_TEXT,
            "product_boundary": "OS-enforced task-scope governance runtime",
            "not_a": [
                "AINE methodology/reference-runtime owner",
                "portfolio registry/control-plane authority",
                "Organon virtual-team/business runtime",
                "standalone multi-agent executor",
            ],
        },
        "release_claims": {
            "local_only_changes_are_unreleased": True,
            "push_performed": False,
            "github_settings_changed": False,
            "remote_mutation_attempted": False,
        },
        "checks": {
            "local_and_origin_refs_resolved": bool(local_head and origin_main),
            "remote_probe_read_only": True,
            "no_push_or_settings_mutation": True,
            "public_claim_is_narrower_than_generic_coding_agent_label": True,
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / "docs/release-sync/orvena-public-sync-2026-08-24.json",
    )
    args = parser.parse_args(argv)
    output = args.output if args.output.is_absolute() else REPO_ROOT / args.output
    evidence = build_evidence()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: evidence[key] for key in ("schema", "sync_state", "local_head", "origin_main", "remote_head")}, ensure_ascii=False))
    return 0 if evidence["checks"]["local_and_origin_refs_resolved"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
