#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGE="$ROOT/target/flatpak-stage"
FLATPAK_DIR="$ROOT/packaging/flatpak"
STATE_DIR="$FLATPAK_DIR/.flatpak-builder"
BUILD_DIR="$FLATPAK_DIR/build-dir"
REPO_DIR="$FLATPAK_DIR/repo"
BUNDLE_OUT="${1:-$ROOT/target/kwhale-desktop.flatpak}"
CARGO_HOME_HOST="${CARGO_HOME:-$HOME/.cargo}"

echo "==> Ensuring workspace Cargo.lock and crate cache via org.kde.Sdk//6.11"
flatpak run --share=network --command=bash \
  --filesystem="$ROOT" \
  --filesystem="$CARGO_HOME_HOST" \
  org.kde.Sdk//6.11 -c "
    export PATH=/usr/lib/sdk/rust-stable/bin:\$PATH CARGO_HOME='$CARGO_HOME_HOST'
    cd '$ROOT'
    cargo fetch --manifest-path '$ROOT/Cargo.toml'
  "

echo "==> Staging clean source tree in $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE/.cargo"
cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$STAGE/"
cp -a "$ROOT/crates" "$STAGE/"
cp -a "$ROOT/assets" "$STAGE/"
mkdir -p "$STAGE/packaging"
cp -a "$ROOT/packaging/flatpak" "$STAGE/packaging/"
rm -rf "$STAGE/packaging/flatpak/build-dir" "$STAGE/packaging/flatpak/repo" "$STAGE/packaging/flatpak/.flatpak-builder"

echo "==> Vendoring Cargo dependencies inside org.kde.Sdk//6.11"
flatpak run --share=network --command=bash \
  --filesystem="$ROOT" \
  --filesystem="$CARGO_HOME_HOST" \
  org.kde.Sdk//6.11 -c "
    export PATH=/usr/lib/sdk/rust-stable/bin:\$PATH CARGO_HOME='$CARGO_HOME_HOST'
    cd '$STAGE'
    cargo vendor --locked vendor > '$STAGE/.cargo/config.toml'
  "

echo "==> Building Flatpak with org.flatpak.Builder"
mkdir -p "$STATE_DIR" "$REPO_DIR" "$(dirname "$BUNDLE_OUT")"
flatpak run --filesystem="$ROOT" org.flatpak.Builder \
  --disable-rofiles-fuse \
  --force-clean \
  --jobs=6 \
  --state-dir="$STATE_DIR" \
  --repo="$REPO_DIR" \
  "$BUILD_DIR" \
  "$FLATPAK_DIR/org.dueattendant149.Kwhale.yml"

echo "==> Exporting single-file .flatpak bundle to $BUNDLE_OUT"
flatpak build-bundle "$REPO_DIR" "$BUNDLE_OUT" org.dueattendant149.Kwhale

echo "==> Installing user Flatpak from bundle"
flatpak install --user -y --reinstall "$BUNDLE_OUT"

echo "==> Done: $BUNDLE_OUT"
