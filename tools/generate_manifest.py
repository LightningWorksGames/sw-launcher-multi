#!/usr/bin/env python3
"""
Generate a file_manifest.json for a Unity build directory.

Usage:
    python generate_manifest.py <build_dir> [--output <path>]

Examples:
    # Generate manifest for a macOS ARM64 build
    python generate_manifest.py ~/Builds/macos-arm64

    # Generate with custom output path
    python generate_manifest.py ~/Builds/windows-x86_64 --output ./manifests/file_manifest.json

The manifest is written into the build directory as file_manifest.json by default.

Expected build server layout (push the entire platform folder):

    SiegeWorldsBuild/
    ├── macos-arm64/
    │   ├── file_manifest.json          ← generated
    │   └── Siege Worlds.app/
    │       └── Contents/...
    ├── macos-x86_64/
    │   ├── file_manifest.json          ← generated
    │   └── Siege Worlds.app/
    │       └── Contents/...
    ├── windows-x86_64/
    │   ├── file_manifest.json          ← generated
    │   ├── Siege Worlds.exe
    │   ├── Siege Worlds_Data/
    │   └── UnityPlayer.dll
    └── linux-x86_64/
        ├── file_manifest.json          ← generated
        ├── Siege Worlds.x86_64
        └── Siege Worlds_Data/

The launcher detects the user's platform, fetches
  {build_server_url}/{platform}/file_manifest.json
and downloads files from
  {build_server_url}/{platform}/{path}
"""

import hashlib
import json
import os
import sys
from pathlib import Path


def sha256_file(filepath: Path) -> str:
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def generate_manifest(build_dir: Path) -> list[dict]:
    entries = []
    for root, _dirs, files in os.walk(build_dir):
        for name in sorted(files):
            filepath = Path(root) / name
            # Skip the manifest file itself and hidden files
            if name == "file_manifest.json" or name.startswith("."):
                continue
            rel_path = filepath.relative_to(build_dir)
            # Normalize to forward slashes for cross-platform compatibility
            rel_str = str(rel_path).replace("\\", "/")
            size = filepath.stat().st_size
            file_hash = sha256_file(filepath)
            entries.append({
                "path": rel_str,
                "hash": file_hash,
                "size": size,
            })
    return entries


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <build_dir> [--output <path>]")
        print()
        print("Generates file_manifest.json for a Unity build directory.")
        sys.exit(1)

    build_dir = Path(sys.argv[1]).resolve()
    if not build_dir.is_dir():
        print(f"Error: {build_dir} is not a directory")
        sys.exit(1)

    # Parse optional --output flag
    output_path = build_dir / "file_manifest.json"
    if "--output" in sys.argv:
        idx = sys.argv.index("--output")
        if idx + 1 < len(sys.argv):
            output_path = Path(sys.argv[idx + 1])

    print(f"Scanning: {build_dir}")
    entries = generate_manifest(build_dir)

    total_size = sum(e["size"] for e in entries)
    size_mb = total_size / 1048576

    with open(output_path, "w") as f:
        json.dump(entries, f, indent=2)

    print(f"Wrote {output_path}")
    print(f"  {len(entries)} files, {size_mb:.1f} MB total")


if __name__ == "__main__":
    main()
