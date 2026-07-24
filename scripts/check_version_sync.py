#!/usr/bin/env python3
"""Fail when release-facing package versions drift across ecosystems."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

parser = argparse.ArgumentParser()
parser.add_argument(
    "--print-publishable",
    action="store_true",
    help="print publishable Rust workspace package names after validation",
)
args = parser.parse_args()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"version-sync: {message}")


def first_version(path: Path, pattern: str) -> str:
    match = re.search(pattern, path.read_text(encoding="utf-8"), re.MULTILINE)
    require(match is not None, f"could not read version from {path.relative_to(ROOT)}")
    return match.group(1)


workspace_version = first_version(
    ROOT / "Cargo.toml",
    r"^version\s*=\s*\"([^\"]+)\"$",
)
metadata = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        text=True,
    )
)
workspace_packages = [
    package for package in metadata["packages"] if package["name"].startswith("samkhya-")
]
# Bump deliberately when a crate is added or removed. The point of pinning the
# count is that growing the workspace should be a conscious act, not something
# that slips in unnoticed and silently escapes the version-sync check below.
# 15 as of 1.2.0: the original 13 plus samkhya-wasm and samkhya-qdrant.
EXPECTED_WORKSPACE_PACKAGES = 15
require(
    len(workspace_packages) == EXPECTED_WORKSPACE_PACKAGES,
    f"expected {EXPECTED_WORKSPACE_PACKAGES} workspace packages, "
    f"found {len(workspace_packages)}: "
    + ", ".join(sorted(p["name"] for p in workspace_packages)),
)
for package in workspace_packages:
    require(
        package["version"] == workspace_version,
        f"{package['name']} is {package['version']}, expected {workspace_version}",
    )

python_version = first_version(
    ROOT / "samkhya-py" / "pyproject.toml",
    r"^version\s*=\s*\"([^\"]+)\"$",
)
node_package = json.loads(
    (ROOT / "samkhya-gpudb" / "scripts" / "package.json").read_text(encoding="utf-8")
)
node_lock = json.loads(
    (ROOT / "samkhya-gpudb" / "scripts" / "package-lock.json").read_text(encoding="utf-8")
)
fuzz_version = first_version(
    ROOT / "samkhya-core" / "fuzz" / "Cargo.lock",
    r'^name = "samkhya-core"\nversion = "([^"]+)"$',
)

observed = {
    "Python project": python_version,
    "Node package": node_package["version"],
    "Node lock root": node_lock["version"],
    "Node lock package": node_lock["packages"][""]["version"],
    "fuzz lock samkhya-core": fuzz_version,
}
for surface, version in observed.items():
    require(version == workspace_version, f"{surface} is {version}, expected {workspace_version}")

if args.print_publishable:
    for package in sorted(workspace_packages, key=lambda item: item["name"]):
        if package["publish"] != []:
            print(package["name"])
else:
    print(
        f"version-sync: {workspace_version} across {len(workspace_packages)} Rust packages, "
        "Python, Node, and fuzz lock"
    )
