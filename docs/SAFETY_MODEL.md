# FolderFlow — Filesystem Safety Model

> The most important document in the repo. Any change to `engine/safe_fs`, `organizer`, `operations`, `undo` or `recovery` must be checked against every invariant here, and the Phase 4 audit checklist re-run.

## Invariants

| ID | Invariant | Enforced by |
|---|---|---|
| S1 | **No silent overwrite.** No operation ever replaces an existing file at the destination. | `rename_excl` (`renamex_np(RENAME_EXCL)`), `copy_excl` (`O_CREAT\|O_EXCL`), clippy ban on `std::fs::rename/copy/write/File::create` outside `safe_fs`. Tests R-SFS, R-CONF. |
| S2 | **No loss.** At every instant, each user file exists at its source, its destination, or both — never neither. | Ordered pipeline (below); fault-injection tests at every step (R-RECOV, R-FAIL). |
| S3 | **No permanent delete** except removing the source after a *verified* cross-volume copy. Everything else uses Trash. | `safe_fs` exposes no generic delete; `remove_verified_source(op)` requires a `Verified` token type. |
| S4 | **Write-ahead intent.** The ledger row (`PENDING`) is durably committed before the first filesystem mutation. | `organizer` API: `Ledger::begin()` returns the token needed to call `safe_fs`. |
| S5 | **Dry-run is read-only.** Planning cannot mutate. | Planner takes `&dyn ReadOnlyFs`; `safe_fs` types not in scope; snapshot test R-PLAN-01. |
| S6 | **No stale execution.** A plan item executes only if identity (dev, ino), size and mtime still match and the destination is still free/valid. | Revalidation in `organizer` (R-EXEC). |
| S7 | **Stay inside approved roots.** Every source and destination is canonicalized and must be under a monitored folder or approved destination root. | `PathAuthority` (R-PATH). |
| S8 | **No symlink escape.** Symlinks are not followed; a symlink source is moved as a link only if a rule explicitly allows it (default: skip). Destination parents must not be symlinks. | `lstat` checks + `O_NOFOLLOW` (R-PATH, R-SFS). |
| S9 | **No unstable files.** Files still being written are not processed. | Stability checker + ignore list (R-STAB). |
| S10 | **No loops.** A file is moved at most once per pass; destinations that re-trigger the same rule are rejected/guarded. | Evaluator limits + recent-move guard (R-EVAL, R-VAL). |
| S11 | **No arbitrary execution.** No shell, script, or command execution exists anywhere. | No `Command` usage (clippy ban), no shell plugin, capability file. |

## Same-volume move

```
1. Ledger: insert op PENDING (identity snapshot)          ── durable commit
2. VALIDATING: lstat source; identity == snapshot; dest parent exists/creatable & inside root;
               dest name free (else conflict policy → new name or skip/review)
3. IN_PROGRESS: mkdir -p dest parent (safe_fs) ; renamex_np(src, dst, RENAME_EXCL)
               EEXIST → conflict policy again (race) ; EXDEV → switch to cross-volume path
4. VERIFYING: lstat dst identity == snapshot; src no longer exists
5. COMPLETED
```
Crash anywhere → recovery checks: does `src` exist? does `dst` exist with snapshot identity? → mark COMPLETED or FAILED (nothing to undo since rename is atomic).

## Cross-volume move (not atomic — never described as such)

```
1. PENDING  (records staging_path = <dst_dir>/.folderflow-<opid>.partial)
2. VALIDATING (as above + free-space check ≥ size + margin)
3. IN_PROGRESS: copy src → staging (O_EXCL, streaming, BLAKE3 while copying); fsync file + dir
4. VERIFYING: re-hash staging == source hash; size match; source identity unchanged
5. rename_excl(staging → dst)                          (atomic on dest volume)
6. remove_verified_source(src)                         (only permanent delete in the app)
7. COMPLETED
```

## Recovery table (startup and after DB reconnect)

| Last state | Filesystem observation | Action |
|---|---|---|
| PENDING / VALIDATING | anything | Nothing was mutated → FAILED (`interrupted_before_start`), file untouched |
| IN_PROGRESS (same-vol) | src exists, dst missing | FAILED, nothing to do |
| IN_PROGRESS (same-vol) | src missing, dst has identity | COMPLETED |
| IN_PROGRESS (cross-vol) | staging exists, src intact | Move staging to Trash; FAILED |
| VERIFYING (cross-vol) | dst verified, src exists | Remove src only if hash re-verifies; else ROLLBACK_REQUIRED |
| VERIFYING | dst verified, src missing | COMPLETED |
| any | both missing | FAILED + **surface to user** (should never happen; P0 bug) |
| ROLLBACK_REQUIRED | — | Show in Activity with explicit user choice; never auto-delete |

Orphan `.folderflow-*.partial` files with no ledger row → moved to Trash, never deleted.

## Undo

- Move/rename undo = a new operation in reverse, through the same pipeline (so it is also excl/no-overwrite).
- Original location occupied → do not overwrite; ask: restore with new name / choose location / cancel.
- Copy undo → trash the copy only if its current hash equals the recorded hash.
- Undo creates `undo_*` operations and sets `reversed_by` on the original.

## Conflict policies

`auto_rename` (default): `name (1).ext`, `name (2).ext`, … chosen *and* claimed atomically via `RENAME_EXCL` retry loop (max 10 000).
`skip`: op FAILED with `conflict_skipped`, reversible = 0.
`ask`: realtime → review queue; interactive preview → dialog.
`keep_both`: same as auto_rename (explicit).
`overwrite`: **only** from an interactive dialog per operation; the existing file is moved to Trash first (not deleted), then rename_excl.

## Ignore list (never processed)

`.DS_Store`, `._*`, `.localized`, `Icon\r`, `.Spotlight-V100`, `.Trashes`, `.fseventsd`, `.TemporaryItems`, `*.download`, `*.crdownload`, `*.part`, `*.partial`, `*.tmp`, `*.opdownload`, `*.!qB`, `.folderflow-*`, any path inside a `.app`/`.bundle`/`.photoslibrary` package, hidden files (if setting on).
