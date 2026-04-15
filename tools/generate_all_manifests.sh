#!/bin/bash
#
# Generate file_manifest.json for all platform build directories.
#
# Usage:
#     ./generate_all_manifests.sh <builds_root>
#
# Where <builds_root> contains platform subdirectories:
#     builds_root/
#     ├── macos-arm64/
#     ├── macos-x86_64/
#     ├── windows-x86_64/
#     └── linux-x86_64/
#
# After running, push the entire builds_root to your build server repo.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILDS_ROOT="${1:?Usage: $0 <builds_root>}"

PLATFORMS=("macos-arm64" "macos-x86_64" "windows-x86_64" "linux-x86_64")

for platform in "${PLATFORMS[@]}"; do
    dir="$BUILDS_ROOT/$platform"
    if [ -d "$dir" ]; then
        echo "=== $platform ==="
        python3 "$SCRIPT_DIR/generate_manifest.py" "$dir"
        echo
    else
        echo "=== $platform === (skipped, directory not found)"
    fi
done

echo "Done. Push $BUILDS_ROOT to your build server."
