#!/usr/bin/env bash
# Builds a release folder for this computer's platform:
#   dist/aidrive-<target>.zip (Windows) or .tar.gz, SHA256SUMS, install.cmd,
#   install.ps1, install.sh, READ-ME-FIRST.txt, licenses and notices.
# The folder is self-contained (PostgreSQL is inside the archive): send it as
# is and run its install.cmd / install.sh, or serve it from any web host for the
# one-line installers. Building on a second platform into the same folder adds
# that archive and recomputes SHA256SUMS for all of them.
# Needs Rust 1.92+ and Node.js 20.9+ (to build the web app once; users need neither).
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)-$(uname -m)" in
    MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64) target=windows-x86_64; ext=.exe ;;
    Linux-x86_64) target=linux-x86_64; ext= ;;
    Linux-aarch64) target=linux-aarch64; ext= ;;
    Darwin-arm64) target=macos-aarch64; ext= ;;
    Darwin-x86_64) target=macos-x86_64; ext= ;;
    *) echo "unsupported platform $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac
dist="${AIDRIVE_DIST_DIR:-dist}"
name="aidrive-$target"
stage="$dist/$name"

# The web export is the same on every platform: AIDRIVE_REUSE_WEB=1 packs the
# apps/web/out of an earlier build, so a Linux build host needs no Node.js.
if [ -n "${AIDRIVE_REUSE_WEB:-}" ] && [ -f apps/web/out/index.html ]; then
    echo "Reusing apps/web/out"
else
    (cd apps/web && { [ node_modules/.package-lock.json -nt package-lock.json ] || npm ci --no-audit --no-fund; } \
        && rm -rf out && AIDRIVE_WEB_EXPORT=1 NEXT_TELEMETRY_DISABLED=1 npx next build --webpack)
fi
cargo build --release --locked -p aidrive-api -p aidrive-mcp-server

rm -rf "$stage" && mkdir -p "$stage"
cp "${CARGO_TARGET_DIR:-target}/release/aidrive-api$ext" "$stage/aidrive$ext"
cp "${CARGO_TARGET_DIR:-target}/release/aidrive-mcp-server$ext" "$stage/"
cp -r apps/web/out "$stage/web"
cp README.md LICENSE-APACHE LICENSE-MIT "$stage/"
[ -f THIRD-PARTY-NOTICES.md ] && cp THIRD-PARTY-NOTICES.md "$stage/"

rm -f "$dist/$name.zip" "$dist/$name.tar.gz"
if [ -n "$ext" ]; then
    archive="$name.zip"
    powershell.exe -NoProfile -Command "Compress-Archive -Path '$(cygpath -w "$stage")\\*' -DestinationPath '$(cygpath -w "$dist/$archive")'"
else
    archive="$name.tar.gz"
    tar -C "$stage" -czf "$dist/$archive" .
fi
rm -rf "$stage"
cp scripts/install.ps1 scripts/install.sh "$dist/"
sed 's/\r*$/\r/' scripts/install.cmd > "$dist/install.cmd"
sed 's/\r*$/\r/' docs/pilot/READ-ME-FIRST.txt > "$dist/READ-ME-FIRST.txt"
cp LICENSE-APACHE LICENSE-MIT THIRD-PARTY-NOTICES.md "$dist/"
(cd "$dist" && sha256sum aidrive-*.zip aidrive-*.tar.gz 2>/dev/null > SHA256SUMS || true)
echo "Release folder: $dist"
ls -l "$dist"
