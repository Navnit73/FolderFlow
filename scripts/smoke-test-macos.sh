#!/usr/bin/env bash
# FolderFlow Packaged .dmg & .app Clean-Environment Smoke Test (P9-T07)
#
# Verifies an actual built DMG & FolderFlow.app bundle without touching real user folders:
#   1. Mounts the .dmg read-only, checks Applications symlink & background assets
#   2. Copies FolderFlow.app into an isolated temp Applications directory
#   3. Verifies universal binary slices, Info.plist metadata & TCC usage strings
#   4. Launches the packaged binary under an isolated temp HOME (first launch -> DB + logs)
#   5. Restarts against the same temp HOME (restart persistence)
#   6. Simulates clean uninstall and verifies user fixture files remain bit-for-bit intact
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DMG_PATH=""
ALLOW_UNSIGNED=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dmg)
      DMG_PATH="${2:-}"
      shift 2
      ;;
    --require-signed)
      ALLOW_UNSIGNED=0
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$DMG_PATH" ]]; then
  DMG_PATH="$(find "$ROOT_DIR/src-tauri/target" -path '*/release/bundle/dmg/FolderFlow_*.dmg' | head -n 1 || true)"
fi

if [[ -z "$DMG_PATH" || ! -f "$DMG_PATH" ]]; then
  echo "error: no built FolderFlow_*.dmg found. Run ./scripts/release-macos.sh first or pass --dmg <path>." >&2
  exit 1
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/folderflow-smoke.XXXXXX")"
MNT_DIR="$WORK_DIR/mnt"
INSTALL_DIR="$WORK_DIR/Applications"
CLEAN_HOME="$WORK_DIR/CleanHome"
mkdir -p "$MNT_DIR" "$INSTALL_DIR" "$CLEAN_HOME/Downloads" "$CLEAN_HOME/Desktop" "$CLEAN_HOME/Library/Application Support" "$CLEAN_HOME/Library/Logs"

ATTACHED=0
APP_PID=""

cleanup() {
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    kill -TERM "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  if [[ "$ATTACHED" -eq 1 ]]; then
    hdiutil detach "$MNT_DIR" -quiet || hdiutil detach "$MNT_DIR" -force -quiet || true
  fi
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

echo "==> Mounting DMG: $DMG_PATH"
hdiutil attach "$DMG_PATH" -nobrowse -readonly -mountpoint "$MNT_DIR" -quiet
ATTACHED=1

if [[ ! -d "$MNT_DIR/FolderFlow.app" ]]; then
  echo "error: FolderFlow.app missing inside DMG" >&2
  exit 1
fi
if [[ ! -L "$MNT_DIR/Applications" ]]; then
  echo "error: /Applications symlink missing inside DMG" >&2
  exit 1
fi
if ! find "$MNT_DIR/.background" -maxdepth 1 -type f -print -quit 2>/dev/null | grep -q .; then
  echo "error: DMG background asset missing inside .background/" >&2
  exit 1
fi

echo "==> Installing FolderFlow.app into isolated directory..."
ditto "$MNT_DIR/FolderFlow.app" "$INSTALL_DIR/FolderFlow.app"
hdiutil detach "$MNT_DIR" -quiet
ATTACHED=0

APP_PATH="$INSTALL_DIR/FolderFlow.app"
INFO_PLIST="$APP_PATH/Contents/Info.plist"
APP_BIN="$APP_PATH/Contents/MacOS/folderflow"

if [[ ! -x "$APP_BIN" ]]; then
  echo "error: executable missing at $APP_BIN" >&2
  exit 1
fi
if [[ ! -f "$APP_PATH/Contents/Resources/icon.icns" ]]; then
  echo "error: icon.icns missing inside FolderFlow.app bundle" >&2
  exit 1
fi

ARCHS="$(lipo -archs "$APP_BIN")"
echo "==> Packaged architectures: $ARCHS"
if [[ "$ARCHS" != *"arm64"* ]]; then
  echo "error: arm64 slice missing in $APP_BIN" >&2
  exit 1
fi
if [[ "$(basename "$DMG_PATH")" == *universal* && "$ARCHS" != *"x86_64"* ]]; then
  echo "error: x86_64 slice missing in universal binary $APP_BIN" >&2
  exit 1
fi

if otool -L "$APP_BIN" | grep -E '(/opt/homebrew|/usr/local/lib|/Users/)'; then
  echo "error: unexpected non-system dynamic library linkage in $APP_BIN" >&2
  exit 1
fi

BUNDLE_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$INFO_PLIST")"
BUNDLE_VER="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INFO_PLIST")"
MIN_OS="$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$INFO_PLIST")"
GRAPHICS_SWITCH="$(/usr/libexec/PlistBuddy -c 'Print :NSSupportsAutomaticGraphicsSwitching' "$INFO_PLIST")"

[[ "$BUNDLE_ID" == "com.folderflow.desktop" ]] || { echo "unexpected CFBundleIdentifier: $BUNDLE_ID" >&2; exit 1; }
[[ "$BUNDLE_VER" == "1.0.0" ]] || { echo "unexpected CFBundleShortVersionString: $BUNDLE_VER" >&2; exit 1; }
[[ "$MIN_OS" == "13.0" ]] || { echo "unexpected LSMinimumSystemVersion: $MIN_OS" >&2; exit 1; }
[[ "$GRAPHICS_SWITCH" == "true" ]] || { echo "NSSupportsAutomaticGraphicsSwitching must be true" >&2; exit 1; }

for key in \
  NSDownloadsFolderUsageDescription \
  NSDesktopFolderUsageDescription \
  NSDocumentsFolderUsageDescription \
  NSRemovableVolumesUsageDescription \
  NSNetworkVolumesUsageDescription; do
  /usr/libexec/PlistBuddy -c "Print :$key" "$INFO_PLIST" >/dev/null
done

if codesign --verify --deep --strict "$APP_PATH" 2>/dev/null; then
  echo "==> Code signature verified"
  if [[ "$ALLOW_UNSIGNED" -eq 0 ]]; then
    spctl --assess -vvv "$APP_PATH"
    xcrun stapler validate "$DMG_PATH"
  fi
else
  if [[ "$ALLOW_UNSIGNED" -eq 0 ]]; then
    echo "error: bundle is unsigned (--require-signed)" >&2
    exit 1
  fi
  echo "==> Bundle is unsigned (PRD Q3 Developer ID signing pending); continuing isolated launch smoke..."
fi

# Seed user files inside isolated clean HOME (never touches real user directories).
printf 'invoice-bytes-v1\n' > "$CLEAN_HOME/Downloads/Invoice_2026.pdf"
printf 'desktop-note-v1\n' > "$CLEAN_HOME/Desktop/Note.txt"
DL_HASH_BEFORE="$(shasum -a 256 "$CLEAN_HOME/Downloads/Invoice_2026.pdf" | awk '{print $1}')"
DESK_HASH_BEFORE="$(shasum -a 256 "$CLEAN_HOME/Desktop/Note.txt" | awk '{print $1}')"

launch_isolated() {
  local run_label="$1"
  echo "==> Launching packaged FolderFlow.app ($run_label) in isolated HOME..."
  local stderr_file="$WORK_DIR/launch-${run_label}.err"
  HOME="$CLEAN_HOME" FOLDERFLOW_LOG=info "$APP_BIN" >"$WORK_DIR/launch-${run_label}.out" 2>"$stderr_file" &
  APP_PID=$!

  local db_file="$CLEAN_HOME/Library/Application Support/com.folderflow.desktop/folderflow.db"
  local log_dir="$CLEAN_HOME/Library/Logs/com.folderflow.desktop"
  local ready=0
  for _ in $(seq 1 30); do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
      echo "error: packaged app exited early during $run_label:" >&2
      cat "$stderr_file" >&2 || true
      return 1
    fi
    if [[ -f "$db_file" ]] && find "$log_dir" -maxdepth 1 -name 'folderflow*.log' -print -quit 2>/dev/null | grep -q .; then
      if grep -q 'FolderFlow started' "$log_dir"/folderflow*.log 2>/dev/null; then
        ready=1
        break
      fi
    fi
    sleep 0.5
  done

  if [[ "$ready" -ne 1 ]]; then
    echo "error: timed out waiting for FolderFlow DB and startup log during $run_label" >&2
    cat "$stderr_file" >&2 || true
    return 1
  fi

  kill -TERM "$APP_PID" 2>/dev/null || true
  wait "$APP_PID" 2>/dev/null || true
  APP_PID=""
}

if [[ -z "${DISPLAY:-}" ]] && ! launchctl managername 2>/dev/null | grep -qiE 'Aqua|Background'; then
  echo "warning: non-Aqua headless session detected; skipping GUI process launch" >&2
else
  launch_isolated "first-launch"
  launch_isolated "restart-launch"

  grep -q 'version="1.0.0"' "$CLEAN_HOME/Library/Logs/com.folderflow.desktop"/folderflow*.log || {
    echo "error: startup log missing version=\"1.0.0\"" >&2
    exit 1
  }
fi

echo "==> Simulating clean uninstall and verifying user files remain intact..."
rm -rf \
  "$APP_PATH" \
  "$CLEAN_HOME/Library/Application Support/com.folderflow.desktop" \
  "$CLEAN_HOME/Library/Logs/com.folderflow.desktop"

DL_HASH_AFTER="$(shasum -a 256 "$CLEAN_HOME/Downloads/Invoice_2026.pdf" | awk '{print $1}')"
DESK_HASH_AFTER="$(shasum -a 256 "$CLEAN_HOME/Desktop/Note.txt" | awk '{print $1}')"
[[ "$DL_HASH_BEFORE" == "$DL_HASH_AFTER" ]] || { echo "Downloads fixture altered!" >&2; exit 1; }
[[ "$DESK_HASH_BEFORE" == "$DESK_HASH_AFTER" ]] || { echo "Desktop fixture altered!" >&2; exit 1; }

echo "✓ Packaged macOS smoke test passed."

