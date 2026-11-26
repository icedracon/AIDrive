#!/bin/sh
# AIDrive installer for Linux and macOS (no root, no Rust or Node).
#   curl -fsSL <release-url>/install.sh | sh
# or, from an extracted release folder: sh <folder>/install.sh
# Run it in your project folder: it installs AIDrive for this user, starts it,
# and connects Claude Code, Codex and OpenCode to a space for that folder.
# Running a newer release the same way updates AIDrive; data is kept.
set -eu

# ---- Release location: the one setting to change when the release is hosted.
# A URL (https://.../download) or a local folder that holds the release files.
DEFAULT_RELEASE="https://github.com/OWNER/aidrive/releases/latest/download"
# ----

here="$(cd "$(dirname "$0")" 2>/dev/null && pwd || true)"
data="${AIDRIVE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/AIDrive}"
bin="${AIDRIVE_INSTALL_DIR:-$data/bin}"
link_dir="${AIDRIVE_LINK_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) target=linux-x86_64 ;;
    Linux-aarch64 | Linux-arm64) target=linux-aarch64 ;;
    Darwin-arm64) target=macos-aarch64 ;;
    Darwin-x86_64) target=macos-x86_64 ;;
    *) echo "AIDrive has no build for $(uname -s) $(uname -m)." >&2; exit 1 ;;
esac
asset="aidrive-$target.tar.gz"
# Run from an extracted release folder, the archive next to this script wins.
if [ -n "${AIDRIVE_RELEASE_URL:-}" ]; then release="$AIDRIVE_RELEASE_URL"
elif [ -n "$here" ] && [ -f "$here/$asset" ]; then release="$here"
else release="$DEFAULT_RELEASE"; fi
release="${release%/}"
case "$release" in
    http://* | https://*) ;;
    *.tar.gz) asset="$(basename "$release")"; release="$(dirname "$release")" ;;
esac
case "$release" in
    */OWNER/*) echo "This installer has no release location. Run install.sh from the extracted AIDrive folder, or set AIDRIVE_RELEASE_URL to that folder." >&2; exit 1 ;;
esac
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() {
    case "$release" in
        http://* | https://*)
            if command -v curl >/dev/null 2>&1; then curl -fsSL "$release/$1" -o "$work/$1"
            else wget -q "$release/$1" -O "$work/$1"; fi ;;
        *) cp "$release/$1" "$work/$1" ;;
    esac
}

echo "Downloading AIDrive from $release"
fetch "$asset"
fetch SHA256SUMS
expected="$(awk -v a="$asset" '$2 == a || $2 == "*" a { print $1; exit }' "$work/SHA256SUMS")"
[ -n "$expected" ] || { echo "SHA256SUMS has no entry for $asset" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$work/$asset" | awk '{print $1}')"
else actual="$(shasum -a 256 "$work/$asset" | awk '{print $1}')"; fi
[ "$actual" = "$expected" ] || { echo "Checksum mismatch for $asset" >&2; exit 1; }

[ -x "$bin/aidrive" ] && "$bin/aidrive" down >/dev/null 2>&1 || true
mkdir -p "$bin"
rm -rf "$bin/web"
# Replace binaries by rename so a running MCP server keeps its old file.
mkdir "$work/x" && tar -xzf "$work/$asset" -C "$work/x"
for f in "$work"/x/*; do
    name="$(basename "$f")"
    if [ -d "$f" ]; then mv "$f" "$bin/$name"; else mv -f "$f" "$bin/$name.new" && mv -f "$bin/$name.new" "$bin/$name"; fi
done
chmod 755 "$bin/aidrive" "$bin/aidrive-mcp-server"
echo "Installed to $bin"

mkdir -p "$link_dir"
ln -sf "$bin/aidrive" "$link_dir/aidrive"
ln -sf "$bin/aidrive-mcp-server" "$link_dir/aidrive-mcp-server"
case ":$PATH:" in
    *":$link_dir:"*) ;;
    *) echo "Add $link_dir to your PATH to run aidrive from any terminal (for example in ~/.profile: export PATH=\"$link_dir:\$PATH\")."
       PATH="$link_dir:$PATH"; export PATH ;;
esac

# Under `curl | sh` stdin is the script, so questions go to the terminal.
if [ -t 1 ] && { : </dev/tty; } 2>/dev/null; then
    "$bin/aidrive" up </dev/tty
else
    "$bin/aidrive" up
fi
# Signs you into the web app with a one-time link; headless computers skip it.
"$bin/aidrive" open 2>/dev/null || echo "Open the web app later with: aidrive open (or aidrive open --no-browser)"
