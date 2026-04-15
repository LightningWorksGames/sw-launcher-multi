# Siege Worlds Launcher: Build & Publish Guide for Unity Developers

## Overview

The Siege Worlds Launcher auto-detects each user's operating system and CPU architecture, then downloads the correct game build for their platform. Unity developers need to build for each target platform, generate a manifest file, and push to the build server repo.

The build server repo is:
**https://github.com/LightningWorksGames/SiegeWorldsBuild** (branch: `main`)

The launcher reads from:
`https://raw.githubusercontent.com/LightningWorksGames/SiegeWorldsBuild/main`

---

## Supported Platforms

| Platform folder    | OS + Architecture       | Unity Build Settings                          |
|--------------------|-------------------------|-----------------------------------------------|
| `macos-arm64`      | macOS Apple Silicon     | Target Platform: macOS, Architecture: ARM64   |
| `macos-x86_64`     | macOS Intel             | Target Platform: macOS, Architecture: x86_64  |
| `windows-x86_64`   | Windows 64-bit          | Target Platform: Windows, Architecture: x86_64|
| `linux-x86_64`     | Linux 64-bit            | Target Platform: Linux, Architecture: x86_64  |

---

## Build Server Repo Structure

The repo must have one folder per platform at the top level. Each folder contains the Unity build output and a generated `file_manifest.json`.

```
SiegeWorldsBuild/
│
├── macos-arm64/
│   ├── file_manifest.json                    <-- generated (see Step 3)
│   └── Siege Worlds.app/
│       └── Contents/
│           ├── MacOS/Siege Worlds
│           ├── Resources/Data/...
│           └── Info.plist
│
├── macos-x86_64/
│   ├── file_manifest.json
│   └── Siege Worlds.app/
│       └── Contents/...
│
├── windows-x86_64/
│   ├── file_manifest.json
│   ├── Siege Worlds.exe
│   ├── UnityPlayer.dll
│   ├── MonoBleedingEdge/...
│   └── Siege Worlds_Data/
│       ├── Managed/...
│       ├── Resources/...
│       └── ...
│
└── linux-x86_64/
    ├── file_manifest.json
    ├── Siege Worlds.x86_64
    ├── UnityPlayer.so
    └── Siege Worlds_Data/...
```

---

## How to Publish a Build (Step by Step)

### Step 1: Build in Unity

For each platform you're updating, do a Unity build:

1. Open the Unity project
2. Go to **File > Build Settings**
3. Select the target platform and architecture (see table above)
4. Set the build output to a clean directory (e.g. `~/UnityBuilds/Windows/`)
5. Click **Build**
6. Repeat for each platform you're updating

**Important:** The game executable must be named **"Siege Worlds"** (the launcher looks for this name). In Unity's Build Settings, set the product name to "Siege Worlds".

### Step 2: Copy builds into the repo

Clone or pull the latest build server repo, then copy each platform's build output into the correct folder:

```bash
# Pull latest
cd ~/SiegeWorldsBuild
git pull

# Copy builds (adjust source paths to match your Unity output)
cp -r ~/UnityBuilds/MacOS-ARM64/*   ~/SiegeWorldsBuild/macos-arm64/
cp -r ~/UnityBuilds/MacOS-Intel/*   ~/SiegeWorldsBuild/macos-x86_64/
cp -r ~/UnityBuilds/Windows/*       ~/SiegeWorldsBuild/windows-x86_64/
cp -r ~/UnityBuilds/Linux/*         ~/SiegeWorldsBuild/linux-x86_64/
```

### Step 3: Generate manifests

The manifest generator script is in the launcher repo at `tools/generate_manifest.py`. It scans a build folder, computes the SHA-256 hash and file size of every file, and writes `file_manifest.json`.

**Option A: All platforms at once**

```bash
# From the launcher repo directory
./tools/generate_all_manifests.sh ~/SiegeWorldsBuild
```

Output:
```
=== macos-arm64 ===
Scanning: /Users/you/SiegeWorldsBuild/macos-arm64
Wrote /Users/you/SiegeWorldsBuild/macos-arm64/file_manifest.json
  83 files, 412.7 MB total

=== windows-x86_64 ===
Scanning: /Users/you/SiegeWorldsBuild/windows-x86_64
Wrote /Users/you/SiegeWorldsBuild/windows-x86_64/file_manifest.json
  147 files, 823.4 MB total
...
```

**Option B: One platform only**

```bash
python3 tools/generate_manifest.py ~/SiegeWorldsBuild/windows-x86_64
```

### Step 4: Commit and push

```bash
cd ~/SiegeWorldsBuild
git add .
git commit -m "Update game build v0.X.X"
git push
```

That's it. Players who click "Check Updates" or "Play" in the launcher will automatically receive the new files.

---

## You Don't Need to Build All Four Every Time

If you only changed the Windows build:

1. Copy the new Windows output into `windows-x86_64/`
2. Run `python3 tools/generate_manifest.py ~/SiegeWorldsBuild/windows-x86_64`
3. Commit and push

The other platforms keep their existing manifests and files untouched.

---

## What the Manifest Looks Like

The generated `file_manifest.json` is a JSON array. Example:

```json
[
  {
    "path": "Siege Worlds.exe",
    "hash": "a1b2c3d4e5f6...",
    "size": 654336
  },
  {
    "path": "Siege Worlds_Data/resources.assets",
    "hash": "f7e8d9c0b1a2...",
    "size": 52428800
  }
]
```

| Field  | Description |
|--------|-------------|
| `path` | File path relative to the platform folder. Uses forward slashes. |
| `hash` | SHA-256 hash (lowercase hex). The launcher uses this to detect which files changed and need re-downloading. |
| `size` | File size in bytes. The launcher uses this to show download progress (e.g. "347.2 MB remaining"). |

**Never edit the manifest by hand.** Always regenerate it with the script after changing any files.

---

## What the Player Sees

When a player opens the launcher:

1. The console shows their platform: `"Platform: macOS (Apple Silicon)"`
2. Clicking **Check Updates** shows: `"21 files need updating (347.2 MB, 83 already up to date)"`
3. Clicking **Play** downloads only the changed files, showing:
   - Current filename above the progress bar
   - Bytes downloaded vs total (e.g. "124.5 MB / 347.2 MB")
   - Percentage progress bar
4. After download completes, the game launches automatically

Files that haven't changed (matching hash) are skipped. Deleted files are cleaned up automatically.

---

## How the Launcher Finds the Right Build

The launcher's build server URL is set to:
`https://raw.githubusercontent.com/LightningWorksGames/SiegeWorldsBuild/main`

When a player on macOS Apple Silicon clicks Play:

1. Launcher detects platform: `macos-arm64`
2. Fetches: `.../main/macos-arm64/file_manifest.json`
3. Downloads files from: `.../main/macos-arm64/{path}`
4. Installs to the player's local install directory

If no platform folder exists yet (e.g. you haven't published a Linux build), the launcher falls back to a `file_manifest.json` in the repo root (legacy mode).

---

## Requirements

- **Python 3.9+** on the machine where you run the manifest generator
- **Git** to push to the SiegeWorldsBuild repo
- **Git LFS** recommended if individual game files exceed 100 MB (GitHub's file size limit for regular git). If files are over 100 MB, talk to the team about setting up Git LFS or switching to an alternative file host (S3, Cloudflare R2, etc.)

---

## Troubleshooting

**"Manifest server returned 404"** — The platform folder or manifest doesn't exist in the repo yet. Make sure you pushed the correct folder name (e.g. `windows-x86_64`, not `Windows` or `win64`).

**"hash mismatch"** — The file on the server doesn't match what the manifest says. Regenerate the manifest and push again. This usually means you updated a file but forgot to re-run the manifest generator.

**Player says "All files are up to date" but game doesn't work** — The manifest has no hash for some entries, so the launcher assumes existing files are fine. Always use the generator script, which includes hashes for everything.
