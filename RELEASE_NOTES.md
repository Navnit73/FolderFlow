# FolderFlow 1.0.0 Release Notes

**Release date:** 2026-11-14  
**Platform:** macOS 13 Ventura and later (Apple Silicon `arm64` + Intel `x86_64` Universal Binary)  
**Distribution:** Direct `.dmg` installer (`com.folderflow.desktop`)

---

## Highlights

FolderFlow **1.0.0** is an offline-first, privacy-first automatic file organizer for macOS built on a zero-overwrite Rust filesystem core and a native Tauri 2 + React interface.

### Safety & Reliability
- **Zero-overwrite primitives:** Same-volume moves use atomic `renamex_np(RENAME_EXCL)`; cross-volume moves stream to a staged temporary file, fsync, verify size + BLAKE3 hash, exclusively rename into place, and only then unlink the source.
- **Write-ahead operation ledger & crash recovery:** Every filesystem mutation commits an intent row before touching disk and automatically recovers or rolls back non-terminal operations on startup.
- **Full Undo & Needs Review queue:** Undo individual operations, selections, or entire batches (including empty folder cleanup), resolve name conflicts interactively when policy is set to **Ask**, or safely skip stale plans.
- **Dry-run Preview:** Preview every matched rule, destination, and potential conflict before moving a single file (`0` filesystem mutations during dry-run).

### Organization, Automation & macOS Integration
- **Continuous multi-folder monitoring:** FSEvents monitoring with debounce, consecutive size+mtime stability checks (ignoring partial browser downloads), automatic periodic & wake reconciliation, and external volume unmount/remount recovery.
- **Visual Rule Builder & Smart Organizers:** Nested `ALL` / `ANY` / `NOT` conditions (up to depth 5), linear-time regex, decimal storage units (`1 KB = 1000 B`), timezone/DST-aware date rules, validated dynamic path templates, and disabled-by-default presets for Screenshots, Invoices & Receipts, Downloads, and Old Files.
- **Streaming Duplicate Finder:** Size prefilter → 3-window partial BLAKE3 → full 64 KiB streaming BLAKE3 with `(dev, ino, size, mtime_ns)` cache invalidation, Review Folder isolation, Trash-with-confirmation, and CSV/JSON export.
- **Scheduler, Menu Bar & Shortcuts:** Interval, daily, weekly, on-launch, and manual schedules with quiet hours and bounded catch-up; live menu bar status tray (`Active`, `Paused`, `Working`, `Error`); `SMAppService` Launch at Login; native `UNUserNotificationCenter` batched notifications; macOS Keychain storage for offline Ed25519 licenses; and full keyboard shortcuts (`⌘N`, `⇧⌘O`, `⌘O`, `⌘F`, `⌘Z`, `⌘R`, `⌘,`, `⌘↩`).
- **WCAG 2.1 AA Accessible UI:** System, Light, and Dark themes with Comfortable and Compact densities verified across all routes with zero accessibility violations.

---

## Installation & Verification

1. Download `FolderFlow_1.0.0_universal.dmg` and verify its checksum against `SHA256SUMS.txt`:
   ```bash
   shasum -a 256 -c SHA256SUMS.txt
   ```
2. Open the `.dmg` and drag **FolderFlow.app** into **Applications**.
3. Launch FolderFlow from **Applications** and complete the non-mutating 6-step Onboarding wizard.

> **Signing & Notarization Note (`Q3`):** Hardened Runtime (`Entitlements.plist`) and signing/notarization automation (`scripts/release-macos.sh`, `.github/workflows/release.yml`) are configured in v1.0.0. Builds produced without Apple Developer ID credentials (`Q3`) are unsigned/ad-hoc; signed and notarized public releases require configuring `APPLE_SIGNING_IDENTITY` and Apple notarization credentials.

---

## Documentation

- End-user guide: [docs/USER_GUIDE.md](docs/USER_GUIDE.md)
- Changelog: [docs/CHANGELOG.md](docs/CHANGELOG.md)
- Architecture & Safety Model: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/SAFETY_MODEL.md](docs/SAFETY_MODEL.md)

