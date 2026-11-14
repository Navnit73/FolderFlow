# FolderFlow User Guide

**Version:** 1.0.0 · **Platform:** macOS 13 Ventura and later (Apple Silicon & Intel Universal)

FolderFlow is an offline-first automatic file organizer for macOS. Every file scan, rule evaluation, duplicate hash, and organization move happens locally on your Mac—your files, filenames, and paths are never uploaded.

---

## 1. Installation

1. Open the downloaded **`FolderFlow_1.0.0_universal.dmg`** disk image.
2. Drag **FolderFlow.app** onto the **Applications** folder icon inside the installer window.
3. Eject the disk image and open **FolderFlow** from **Applications** or Spotlight.

> **Note on Gatekeeper (unsigned preview builds vs. Developer ID builds):** Signed and notarized releases open directly on first launch. If you are testing a local unsigned build before Developer ID signing (`Q3`) is enabled, Control-click **FolderFlow.app** in Finder and choose **Open** (or allow it in **System Settings → Privacy & Security**).

---

## 2. First Launch & Onboarding (Zero-Write Guarantee)

On first launch, FolderFlow walks through a 6-step onboarding wizard:

1. **Welcome** — Explains local-only operation and user control.
2. **Choose Folders** — Select **Downloads**, **Desktop**, **Documents**, or custom folders.
3. **macOS Folder Access (TCC)** — macOS will prompt for folder access. If access is denied, FolderFlow provides a direct **Open System Settings** link to **Privacy & Security → Files and Folders** plus a **Check Again** action.
4. **Organization Preference** — Choose how you prefer to sort files (by category, date, type, custom rules, or keeping structure).
5. **Starter Rules** — Pick optional templates (Invoices & Receipts, Screenshots, PDFs, Images, Videos, Archives, Old Downloads).
6. **Dry-Run Scan & Confirmation** — FolderFlow scans your selected folders and displays what would match **without moving, renaming, or modifying a single file**. Starter rules remain **disabled** unless you explicitly check the confirmation box to enable them.

You can re-run the setup wizard anytime from **Settings → General → Re-run setup wizard**.

---

## 3. Monitored Folders & Stability Safety

Open **Folders** in the sidebar to manage watched directories:

- **Controls:** Pause, resume, or trigger **Rescan** per folder or globally.
- **Scope & Exclusions:** Toggle **Include subfolders**, restrict **Only extensions** or **Skip extensions**, exclude specific subfolders, or override the **Stability interval**.
- **Partial Download Protection:** FolderFlow automatically ignores hidden files (configurable), `.DS_Store`, `.localized`, AppleDouble (`._*`), `.app` bundle internals (treated as single items), and active browser/download partials (`.download`, `.crdownload`, `.part`, `.partial`, `.tmp`, `.opdownload`).
- **Stability Checks:** A newly detected file is only eligible for automation after its size and modification time remain unchanged across multiple consecutive checks.
- **External Drives & Sleep/Wake:** Disconnected drives transition cleanly to **Unavailable** and resume automatically when remounted. Waking your Mac triggers automatic reconciliation so missed events are never lost.

---

## 4. Rules, Templates & Smart Organizers

Open **Rules** to view priority-ordered rules, toggle enable switches, drag/reorder priority, duplicate, or delete rules.

### Rule Builder
- **Condition Groups:** Combine `ALL` (AND), `ANY` (OR), and `NOT` groups up to 5 levels deep.
- **Conditions:** File name/stem/extension/MIME/path, category, decimal file size (`1 KB = 1000 B`), timezone-aware dates (`created`, `modified`, `added`), location, filename globs, linear-time regex, hidden/empty status, duplicate status, and BLAKE3 content hash.
- **Safe Actions:** Move, move into dynamic folder, copy, rename, add prefix/suffix, remove/replace text, change extension, add date, skip, stop processing, notify, mark for review, or apply another rule (with cycle and loop detection). FolderFlow never executes shell commands or scripts.
- **Dynamic Templates:** Use `{year}`, `{month}`, `{month_name}`, `{day}`, `{date}`, `{extension}`, `{category}`, `{original_name}`, and `{source_folder}`. All generated paths are checked against approved roots with traversal protection and live preview before saving.
- **Live Rule Test:** Click **Test on folder** inside the Rule Builder to run a dry-run match test on any monitored folder.

### Smart Organizers
Built-in templates (all installed disabled until you review them):
- **Screenshots** (matches macOS `Screenshot`, `Screen Shot`, and `CleanShot` names)
- **Invoices & Receipts** (filename keyword matching: `invoice`, `receipt`, `bill`, `payment`, `tax`, `gst`)
- **Clean Downloads** (per-category presets)
- **Old Files Archiver** (7 / 15 / 30 / 60 / 90 / 180 / 365 days — archives, never deletes)

---

## 5. Preview (Dry-Run) & Execution

Always inspect changes before automating:
1. Open **Preview** (`⌘R` refreshes).
2. Filter by folder, rule, category, or search filenames.
3. Inspect conflicts (duplicate destinations, permission warnings, unavailable destinations, rule conflicts).
4. Deselect any items you want to skip, pick a conflict policy (**Auto-Rename**, **Skip**, **Ask / Needs Review**, or **Keep Both**), and click **Execute Selected** (or press `⌘↩`).

Before every file executes, FolderFlow revalidates the file's `(dev, inode)`, size, modification time, and destination path. If the file changed since the preview was built, it is safely skipped and reported as stale.

---

## 6. Activity Log, Undo & Recovery

Open **Activity** to audit every operation:

- **No Silent Overwrites:** FolderFlow uses macOS `renamex_np(RENAME_EXCL)` for same-volume moves and staged BLAKE3-verified copies for cross-volume moves.
- **Undo:** Undo the last operation (`⌘Z` outside text inputs), selected rows, or an entire batch (**Restore folder structure** removes empty folders created during that batch). If the original path is now occupied by a newer file, FolderFlow alerts you instead of overwriting it.
- **Needs Review:** Items matched under the **Ask** conflict policy appear in the **Needs Review** queue where you can choose **Keep Both** or **Skip**.
- **Crash Recovery:** If FolderFlow quits mid-operation, startup recovery checks the write-ahead SQLite ledger before watchers start and completes or rolls back interrupted moves cleanly, moving any orphaned `.partial` staging files to Trash.
- **Export:** Export filtered activity logs to CSV (with spreadsheet formula-injection protection).

---

## 7. Duplicates

Open **Duplicates** to find identical files safely:
1. FolderFlow groups files by **size**, runs a 3-window **partial BLAKE3** check, and confirms matches with a **64 KiB streaming BLAKE3** full hash (bounded memory, cached by `(dev, ino, size, mtime_ns)`).
2. Choose which file to keep, **Reveal in Finder**, move duplicates to a dedicated **Review Folder** (automatically excluded from future scans), **Move to Trash** (only with explicit confirmation—never permanent deletion), **Ignore**, or export a CSV/JSON duplicate report.

---

## 8. Scheduler, Menu Bar & Keyboard Shortcuts

- **Scheduler (`/scheduler`):** Run specific rules or folders on intervals (5/15/30 min, hourly, daily, weekly), on launch, or manually—with **Quiet Hours** and **Skip** / **Run Once** catch-up policies when your Mac wakes from sleep.
- **Menu Bar Tray:** Closing the main window keeps FolderFlow monitoring in the menu bar (`Active`, `Paused`, `Working`, `Error`). Use the tray menu to pause/resume monitoring, run **Organize Now**, open **Preview Pending**, view **Recent Activity**, open **Settings**, or **Quit FolderFlow**.
- **Shortcuts:**
  - `⌘N` — New rule
  - `⇧⌘O` — Add monitored folder
  - `⌘O` — Open Folders screen
  - `⌘F` — Focus search field
  - `⌘Z` — Undo last operation (disabled while typing in inputs)
  - `⌘R` — Refresh active view
  - `⌘,` — Open Settings
  - `⌘↩` — Execute selected items on Preview

---

## 9. Settings, Backup & Privacy

- **General:** Launch at Login (`SMAppService`), auto-start monitoring, start minimized to menu bar, show menu bar icon, confirm bulk operations, re-run onboarding.
- **Appearance:** System / Light / Dark themes; Comfortable / Compact density.
- **Backup (`/settings/backup`):** Export all folders, rules, custom categories, exclusions, and settings as a versioned JSON file; preview structural diffs before importing (**Merge** or **Replace**). Imported rules always start **disabled** for safety.
- **Privacy & Logs:** Diagnostic logs live in `~/Library/Logs/com.folderflow.desktop/` with daily rotation and automatic path redaction enabled by default. FolderFlow never logs file contents.

---

## 10. License Activation (Offline-First)

Open **License** in the sidebar footer to activate FolderFlow:
- Paste an offline Ed25519-signed `FF1.*` license key or open a `.fflicense` file.
- Your license key is stored securely in your **macOS Keychain** (`com.folderflow.desktop.license`).
- FolderFlow validates offline keys cryptographically on launch with **zero network calls**. Monitoring and rule execution never depend on an external license server.

---

## 11. Uninstalling FolderFlow (Your Files Stay Untouched)

Removing FolderFlow **never moves, modifies, or deletes any of your personal files or folders**:

1. Click the FolderFlow menu bar icon and choose **Quit FolderFlow** (if running).
2. If **Launch at Login** was enabled, turn it off in **Settings → General** (or **System Settings → General → Login Items**).
3. Drag **FolderFlow.app** from **Applications** to the **Trash**.
4. *(Optional)* To remove FolderFlow's database, settings, and diagnostic logs, delete:
   - `~/Library/Application Support/com.folderflow.desktop/`
   - `~/Library/Logs/com.folderflow.desktop/`
   - *(Optional)* Remove the `com.folderflow.desktop.license` entry in **Keychain Access** if desired.

All monitored folders (Downloads, Desktop, Documents, external drives) and all organized files remain completely untouched where you left them.

---

## 12. Clean macOS VM Verification Checklist (Release QA — `P9-T07`)

For verifying a packaged `.dmg` on clean **macOS 13 Ventura** and **latest macOS** virtual machines:

```bash
# Run automated DMG mount, universal slice check, isolated HOME launch,
# restart persistence, and uninstall user-file preservation verification:
./scripts/smoke-test-macos.sh --dmg path/to/FolderFlow_1.0.0_universal.dmg
```

Interactive GUI checklist on each clean VM:
1. Mount `.dmg`, drag **FolderFlow.app** to `/Applications`, eject DMG, and launch.
2. Complete 6-step Onboarding against test folders (`~/Downloads`, `~/Desktop`); verify TCC prompt appears and zero files move during onboarding.
3. Enable a rule, run **Preview**, execute (`⌘↩`), verify moved file in Finder.
4. Press `⌘Z` (or click **Undo**) and verify file returns to original location.
5. Quit and relaunch FolderFlow; confirm folders, rules, settings, and activity history persist.
6. Disconnect network / keep Wi-Fi off; activate an offline `FF1.*` / `.fflicense` key and confirm organization continues offline.
7. Quit, remove **FolderFlow.app** and `~/Library/{Application Support,Logs}/com.folderflow.desktop`, and verify all user files remain untouched.

