#!/usr/bin/env bash
# FolderFlow macOS Release Builder, Signer, Notarizer & Checksum Generator (P9-T02..T06)
#
# Usage:
#   ./scripts/release-macos.sh [--skip-gates] [--target universal-apple-darwin] [--require-signing]
#
# Optional environment variables for Developer ID signing & notarization (PRD Q3):
#   APPLE_SIGNING_IDENTITY   e.g. "Developer ID Application: Your Name (TEAMID)"
#   APPLE_TEAM_ID            10-character Apple Team ID
#   APPLE_API_KEY            App Store Connect API Key ID (recommended for CI)
#   APPLE_API_ISSUER         App Store Connect API Issuer UUID
#   APPLE_API_KEY_PATH       Path to AuthKey_<ID>.p8 private key file
#   -- or --
#   APPLE_ID                 Apple ID email (with APPLE_PASSWORD app-specific password)
#   APPLE_PASSWORD           App-specific password for notarytool
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="universal-apple-darwin"
SKIP_GATES=0
REQUIRE_SIGNING=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-gates)
      SKIP_GATES=1
      shift
      ;;
    --target)
      TARGET="${2:-}"
      if [[ -z "$TARGET" ]]; then
        echo "error: --target requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    --require-signing)
      REQUIRE_SIGNING=1
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
  esac
done

cd "$ROOT_DIR"

if [[ "$SKIP_GATES" -eq 0 ]]; then
  echo "==> Running frontend and Rust verification gates..."
  pnpm gates
  (
    cd src-tauri
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    PROPTEST_CASES=10000 cargo test --workspace
  )
fi

if [[ "$TARGET" == "universal-apple-darwin" ]]; then
  for req_target in aarch64-apple-darwin x86_64-apple-darwin; do
    if ! rustup target list --installed | grep -qx "$req_target"; then
      echo "==> Installing Rust target $req_target..."
      rustup target add "$req_target"
    fi
  done
fi

TAURI_ARGS=(exec tauri build --target "$TARGET" --bundles app,dmg --ci)

if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  echo "==> Building signed bundle with Developer ID identity..."
  export APPLE_SIGNING_IDENTITY
  if [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
    echo "==> Notarization configured via App Store Connect API Key."
  elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
    echo "==> Notarization configured via Apple ID + app-specific password."
  else
    echo "warning: APPLE_SIGNING_IDENTITY is set, but notarization credentials are incomplete (Q3)." >&2
  fi
  pnpm "${TAURI_ARGS[@]}"
else
  if [[ "$REQUIRE_SIGNING" -eq 1 ]]; then
    echo "error: --require-signing passed, but APPLE_SIGNING_IDENTITY is not set (PRD Q3)." >&2
    exit 1
  fi
  echo "==> APPLE_SIGNING_IDENTITY not set (PRD Q3 pending); building unsigned universal bundle (--no-sign)..."
  pnpm "${TAURI_ARGS[@]}" --no-sign
fi

BUNDLE_DIR="$ROOT_DIR/src-tauri/target/$TARGET/release/bundle"
APP_PATH="$BUNDLE_DIR/macos/FolderFlow.app"

if [[ ! -d "$APP_PATH" ]]; then
  echo "error: expected .app bundle missing at $APP_PATH" >&2
  exit 1
fi

APP_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP_PATH/Contents/Info.plist")"
DMG_PATH="$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name "FolderFlow_${APP_VERSION}_*.dmg" | head -n 1)"

if [[ -z "$DMG_PATH" || ! -f "$DMG_PATH" ]]; then
  echo "error: expected .dmg installer for v${APP_VERSION} missing under $BUNDLE_DIR/dmg" >&2
  exit 1
fi

BIN_PATH="$APP_PATH/Contents/MacOS/folderflow"
ARCHS="$(lipo -archs "$BIN_PATH")"
echo "==> Bundle executable architectures: $ARCHS"
if [[ "$TARGET" == "universal-apple-darwin" ]]; then
  if [[ "$ARCHS" != *"arm64"* || "$ARCHS" != *"x86_64"* ]]; then
    echo "error: universal binary missing required slice(s): $ARCHS" >&2
    exit 1
  fi
fi

if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  echo "==> Verifying Hardened Runtime signature & entitlements..."
  codesign --verify --deep --strict --verbose=2 "$APP_PATH"
  codesign -dvv "$APP_PATH" 2>&1 | grep -q "flags=.*runtime" || {
    echo "error: Hardened Runtime flag not present on $APP_PATH" >&2
    exit 1
  }
  codesign -d --entitlements - "$APP_PATH"
  spctl --assess -vvv "$APP_PATH"

  if xcrun stapler validate "$DMG_PATH" >/dev/null 2>&1; then
    echo "==> Notarization ticket stapled and validated on $DMG_PATH"
  else
    echo "warning: notarization ticket not stapled on $DMG_PATH (check APPLE_* notarization env vars)" >&2
  fi
fi

OUT_DIR="$ROOT_DIR/dist-release"
mkdir -p "$OUT_DIR"
DMG_NAME="$(basename "$DMG_PATH")"
cp -f "$DMG_PATH" "$OUT_DIR/$DMG_NAME"
TAR_NAME="FolderFlow_${APP_VERSION}_${TARGET}.app.tar.gz"
tar -C "$BUNDLE_DIR/macos" -czf "$OUT_DIR/$TAR_NAME" "FolderFlow.app"
(
  cd "$OUT_DIR"
  shasum -a 256 "$DMG_NAME" "$TAR_NAME" > SHA256SUMS.txt
)

echo "==> Release artifacts ready in $OUT_DIR:"
cat "$OUT_DIR/SHA256SUMS.txt"

