# FolderFlow — Phase Audit Process

At the end of **every** phase, re-audit the phase's code *and* re-run all earlier phases' tests. Record the result in `docs/audits/phase-N.md` (copy [audits/TEMPLATE.md](audits/TEMPLATE.md)). A phase cannot be marked ✅ in the tracker without a completed audit.

## Steps

1. **Freeze** — no new features; only fixes during the audit.
2. **Gates** — run every CI gate; paste the summary into the audit file.
3. **Checklist review** — walk every item below that applies to the phase. Read the diff, not just the tests.
4. **Bug hunt** — actively try to break it: edge cases, concurrency, odd filenames (unicode, emoji, spaces, 255-byte names, leading dots, NFD), huge files, empty files, permission denied, missing folders.
5. **Log bugs** — every finding → [BUGS.md](BUGS.md) with severity, a failing regression test, then the fix.
6. **Re-run** — all gates + all previous phases' tests.
7. **Docs sync** — ensure PRD/ARCHITECTURE/DATABASE/SAFETY/TRACKER match the actual code. Drift is a bug.
8. **Sign off** — audit file status → ✅; tracker phase row → ✅.

## Severity

| Sev | Meaning | Rule |
|---|---|---|
| S0 | Possible data loss / overwrite / delete / escape from approved roots | Blocks phase. Fix + regression test + safety-model review. |
| S1 | Wrong behavior, crash, security issue, incorrect preview | Blocks phase. |
| S2 | Degraded UX, perf issue, missing state (empty/error) | Fix in phase, or defer with tracker note. |
| S3 | Cosmetic | May defer. |

## Checklists

### A. Data safety (Phases 2, 4, 5, 6, 7 — mandatory)
- [ ] No mutating FS call outside `engine/safe_fs` (`grep` + clippy).
- [ ] Every mutation is preceded by a committed ledger intent.
- [ ] Every destination goes through `PathAuthority`.
- [ ] No `rename` without `RENAME_EXCL`; no `create` without `O_EXCL`.
- [ ] Recovery table rows all have tests and pass.
- [ ] Dry-run snapshot test passes.
- [ ] Fault injection at every step leaves files at src or dst (never neither).

### B. Correctness
- [ ] Every FR ID in the phase has at least one test.
- [ ] Edge-case filenames tested (unicode NFC/NFD, emoji, spaces, 255 bytes, leading dot, no extension, double extension `.tar.gz`).
- [ ] Time logic uses injected clock; DST and timezone covered.
- [ ] Errors are typed, logged once, surfaced to UI with actionable message.

### C. Concurrency & resources
- [ ] All channels bounded; all worker pools limited.
- [ ] No blocking IO on async executor threads (use `spawn_blocking`).
- [ ] No unbounded `Vec` growth proportional to folder size in hot paths.
- [ ] Locks never held across `.await` on FS calls.

### D. Security
- [ ] Capabilities file unchanged or reviewed.
- [ ] No new command accepts raw paths without validation.
- [ ] No `Command`/shell; no network outside license module.
- [ ] Logs contain no file contents; paths only at debug.
- [ ] `cargo audit` clean; new dependencies justified in DECISIONS.md.

### E. UI / theme
- [ ] `pnpm check:tokens` passes — zero hardcoded colors/sizes outside `theme.css`.
- [ ] Light + dark + compact + comfortable visually checked.
- [ ] Empty, loading, error, success states exist.
- [ ] Keyboard navigable, focus ring visible, labels for icon buttons.
- [ ] No placeholder buttons, no mock data.

### F. Docs
- [ ] Tracker updated (tasks, test counts, status).
- [ ] Architecture/DB/safety docs match code.
- [ ] CHANGELOG entry.
