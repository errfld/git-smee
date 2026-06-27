#!/usr/bin/env python3
"""Render the Homebrew tap formula for git-smee release assets.

The release workflow builds binary tarballs for the Homebrew-supported Unix
platforms, downloads their .sha256 sidecars, and uses this script to write the
tap formula in one deterministic update.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

REPO = "errfld/git-smee"
SUPPORTED_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
)


def asset_name(tag: str, target: str) -> str:
    return f"git-smee-{tag}-{target}.tar.gz"


def version_from_tag(tag: str) -> str:
    if not re.fullmatch(r"v[0-9]+(\.[0-9]+)*([-.][0-9A-Za-z.]+)?", tag):
        raise ValueError(f"invalid release tag: {tag!r}")
    return tag.removeprefix("v")


def load_checksums(tag: str, checksum_dir: Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for target in SUPPORTED_TARGETS:
        asset = asset_name(tag, target)
        checksum_file = checksum_dir / f"{asset}.sha256"
        if not checksum_file.exists():
            continue
        first_line = checksum_file.read_text(encoding="utf-8").splitlines()[0]
        checksum, _, filename = first_line.partition("  ")
        if filename != asset:
            raise ValueError(
                f"checksum file {checksum_file} names {filename!r}, expected {asset!r}"
            )
        if not re.fullmatch(r"[0-9a-fA-F]{64}", checksum):
            raise ValueError(f"invalid sha256 in {checksum_file}: {checksum!r}")
        checksums[asset] = checksum.lower()

    missing = [
        asset_name(tag, target)
        for target in SUPPORTED_TARGETS
        if asset_name(tag, target) not in checksums
    ]
    if missing:
        missing_list = ", ".join(missing)
        raise ValueError(f"missing Homebrew checksum assets: {missing_list}")
    return checksums


def url(tag: str, target: str) -> str:
    return f"https://github.com/{REPO}/releases/download/{tag}/{asset_name(tag, target)}"


def render_formula(tag: str, checksums: dict[str, str]) -> str:
    version = version_from_tag(tag)
    mac_arm = asset_name(tag, "aarch64-apple-darwin")
    mac_intel = asset_name(tag, "x86_64-apple-darwin")
    linux_intel = asset_name(tag, "x86_64-unknown-linux-gnu")

    return f'''class GitSmee < Formula
  desc "Git hook manager"
  homepage "https://github.com/{REPO}"
  version "{version}"

  on_macos do
    on_arm do
      url "{url(tag, "aarch64-apple-darwin")}"
      sha256 "{checksums[mac_arm]}"
    end

    on_intel do
      url "{url(tag, "x86_64-apple-darwin")}"
      sha256 "{checksums[mac_intel]}"
    end
  end

  on_linux do
    on_intel do
      url "{url(tag, "x86_64-unknown-linux-gnu")}"
      sha256 "{checksums[linux_intel]}"
    end
  end

  def install
    bin.install "git-smee"
  end

  test do
    system "#{{bin}}/git-smee", "--help"
  end
end
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True, help="release tag, e.g. v0.0.4")
    parser.add_argument("--checksum-dir", required=True, type=Path)
    parser.add_argument("--formula", required=True, type=Path)
    args = parser.parse_args()

    checksums = load_checksums(args.tag, args.checksum_dir)
    args.formula.write_text(render_formula(args.tag, checksums), encoding="utf-8")


if __name__ == "__main__":
    main()
