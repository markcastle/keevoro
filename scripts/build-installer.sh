#!/usr/bin/env bash
# Builds the desktop app's installer for the computer this runs on and puts
# it in installers/ under a name that carries no version, so "the latest
# Windows installer" is always the same address on a release.
#
#   scripts/build-installer.sh                                  Windows
#   scripts/build-installer.sh --target universal-apple-darwin  macOS, Apple silicon and Intel in one
#
# Used by both the CI workflow (to prove the installers build, and to give
# every change on main one to try) and the Release workflow, so the two
# cannot build differently. Pass the version as KEEVORO_VERSION to have the
# script refuse an installer that says it is anything else.

set -euo pipefail

npx tauri build "$@"

mkdir -p installers
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    built=$(find target -path '*bundle/nsis/*-setup.exe' | head -1)
    name=Keevoro-windows-x64-setup.exe
    ;;
  Darwin)
    built=$(find target -path '*bundle/dmg/*.dmg' | head -1)
    name=Keevoro-macos-universal.dmg
    ;;
  *)
    echo "No installer is made for $(uname -s); the app runs there from source." >&2
    exit 1
    ;;
esac

if [ -z "$built" ]; then
  echo "The build finished but no installer was found under target/." >&2
  exit 1
fi
# Tauri names the file after the version it built, which is the surest
# sign of what is inside.
if [ -n "${KEEVORO_VERSION:-}" ] && [[ "$(basename "$built")" != *"_${KEEVORO_VERSION}_"* ]]; then
  echo "Expected an installer for ${KEEVORO_VERSION} but built $(basename "$built")." >&2
  exit 1
fi
cp "$built" "installers/$name"
echo "Built $(basename "$built") -> installers/$name"
ls -la installers
