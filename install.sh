#!/bin/sh
# Installs or updates Keevoro on macOS or Linux from the latest GitHub Release.
#
#   curl -fsSL https://raw.githubusercontent.com/markcastle/keevoro/main/install.sh | sh
#
# It downloads the build for this computer, checks it against the release's
# checksum file, and puts it in ~/.local/bin (set KEEVORO_INSTALL_DIR to
# choose somewhere else). Nothing needs an administrator password. Running
# it again updates to the newest version; so does `keevoro update`. If the
# repository is ever made private again, set GITHUB_TOKEN to a token that
# can read it.

set -eu

REPO="markcastle/keevoro"
# Only the tests point this somewhere else.
API="${KEEVORO_API:-https://api.github.com}"
DIR="${KEEVORO_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
  Darwin) asset="keevoro-macos-universal" ;;
  Linux)
    case "$(uname -m)" in
      x86_64 | amd64) asset="keevoro-linux-x64" ;;
      *)
        echo "There is no Keevoro build for Linux on $(uname -m) yet, only for x86_64." >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "This installer is for macOS and Linux. On Windows, see the README for the PowerShell one-liner." >&2
    exit 1
    ;;
esac

# fetch <url> <accept> [output file]: a GET with the token when there is one.
fetch() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" -H "Accept: $2" ${3:+-o "$3"} "$1"
  else
    curl -fsSL -H "Accept: $2" ${3:+-o "$3"} "$1"
  fi
}

echo "Looking up the latest Keevoro release..."
json=$(fetch "$API/repos/$REPO/releases/latest" "application/vnd.github+json") || {
  echo "Could not read the latest release of $REPO. Check your internet connection and try again." >&2
  exit 1
}
tag=$(printf '%s' "$json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)

# Each file on a release is listed as {"url": ".../releases/assets/N", ...,
# "name": "..."}. Pair them up and pick the address of the one wanted.
asset_url() {
  printf '%s' "$json" | tr -d '\n' |
    grep -o '"url": *"[^"]*/releases/assets/[0-9]*"[^{}]*"name": *"[^"]*"' |
    grep "\"name\": *\"$1\"" |
    sed 's/^"url": *"\([^"]*\)".*/\1/' |
    head -1
}
url=$(asset_url "$asset")
sums_url=$(asset_url "SHA256SUMS")
if [ -z "$url" ] || [ -z "$sums_url" ]; then
  echo "Release $tag has no build for this computer ($asset)." >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "Downloading Keevoro $tag..."
fetch "$url" "application/octet-stream" "$tmp/keevoro"
fetch "$sums_url" "application/octet-stream" "$tmp/SHA256SUMS"

# Never install a download that does not match the release's checksum.
expected=$(grep " $asset\$" "$tmp/SHA256SUMS" | awk '{print $1}' | head -1)
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/keevoro" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$tmp/keevoro" | awk '{print $1}')
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "The download did not match the release's checksum, so nothing was installed. Try again." >&2
  exit 1
fi

chmod +x "$tmp/keevoro"
if [ "$(uname -s)" = "Darwin" ]; then
  # Downloads can carry a quarantine flag that makes macOS refuse to run
  # them. Clear it, and make sure there is at least an ad-hoc signature,
  # which Apple silicon insists on.
  xattr -d com.apple.quarantine "$tmp/keevoro" 2>/dev/null || true
  if ! codesign -v "$tmp/keevoro" 2>/dev/null; then
    codesign --force --sign - "$tmp/keevoro" >/dev/null 2>&1 || true
  fi
fi

mkdir -p "$DIR"
# Moved into place in one step, so a copy that is running is never left
# half-written.
mv -f "$tmp/keevoro" "$DIR/keevoro"

echo "Keevoro $tag is installed in $DIR."
case ":$PATH:" in
  *":$DIR:"*) echo "Try it:  keevoro --help" ;;
  *)
    echo "One more step: $DIR is not on your PATH yet. Add this line to your shell profile, then open a new terminal:"
    echo "  export PATH=\"$DIR:\$PATH\""
    echo "Until then, run it as:  $DIR/keevoro --help"
    ;;
esac
