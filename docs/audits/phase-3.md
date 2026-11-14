# Phase 3 Audit — Rule engine

| Field | Value |
|---|---|
| Phase | 3 |
| Audit date | 2026-11-14 |
| Status | ✅ Signed off |
| Base commit | `0b9a363` (Phase 3 work uncommitted at audit time) |

## 1. Gate results (final run after all fixes)

| Gate | Result |
|---|---|
| cargo fmt --check | ✅ |
| cargo clippy --all-targets --all-features -D warnings | ✅ 0 issues |
| cargo test | ✅ 156 / 156 — **run 10× consecutively, 10/10 green** |
| proptest 10k cases (`PROPTEST_CASES=10000 cargo test`) | ✅ 30,000+ generated cases across `r_cond_prop_evaluators_never_panic_and_are_deterministic`, `r_grp_05_proptest_boolean_algebra_laws`, and `r_tpl_prop_output_never_contains_traversal_or_invalid_chars` — 0 failures |
| cargo audit | ✅ exit 0 — 0 new crates added to `Cargo.lock` (ADR-015 promoted `chrono`, `glob`, `regex` already in lockfile) |
| pnpm lint / typecheck / build | ✅ |
| pnpm test | ✅ 64 / 64 (9 test files) |
| pnpm check:tokens | ✅ |
| Phase 1–2 tests | ✅ all still pass with zero regressions |

## 2. Requirement coverage

| Req | Implemented by | Tests |
|---|---|---|
| FR-040 rule fields & persistence | `rules/models.rs`, `database/repo_rules.rs`, `rules/service.rs` | R-RULE-01..06, UI-RULES-01 |
| FR-041 condition groups ALL/ANY/NOT, max depth 5 | `rules/conditions.rs`, `rules/validate.rs`, `ConditionGroup.tsx` | R-GRP-01..05, R-VAL-02, UI-RB-03 |
| FR-042 file attribute conditions | `rules/conditions.rs`, `ConditionRow.tsx` | R-COND-01..05, UI-RB-02 |
| FR-043 text operators & regex | `rules/conditions.rs` | R-COND-01..03 |
| FR-044 size operators & decimal units (1 KB = 1000 B) | `SizeUnit::to_bytes`, `ConditionRow.tsx` | R-COND-05, UI-RB-02 |
| FR-045 date conditions in local timezone + DST | `utils/clock.rs`, `rules/conditions.rs` | R-COND-06..10 |
| FR-046 location conditions | `rules/conditions.rs` | R-COND-11..12 |
| FR-047 advanced conditions (hash, duplicate, empty, glob, extension group) | `rules/conditions.rs` | R-COND-04, R-COND-13..15 |
| FR-048 regex validation & size limits (`REGEX_SIZE_LIMIT = 256 KiB`) | `rules/validate.rs`, `rules/conditions.rs` | R-COND-03, R-VAL-01 |
| FR-050 all 16 actions | `rules/models.rs`, `rules/evaluator.rs`, `ActionRow.tsx` | R-RULE-01, R-EVAL-02..06, UI-RB-04 |
| FR-051 no shell/script/command action | `Action` enum, clippy bans | R-RULE-03, R-SEC-02 |
| FR-052 priority ordering & 5 execution modes | `rules/evaluator.rs`, `rules/service.rs` | R-EVAL-01..02, R-RULE-05, UI-RULES-02 |
| FR-053 loop detection (≤1 move/pass, ≤10 actions/pass, `ApplyAnotherRule` cycle & depth ≤5) | `rules/evaluator.rs`, `rules/validate.rs` | R-EVAL-03..04, R-VAL-03/05, BUG-016 |
| FR-054 destination inside monitored source folder flagged & guarded | `rules/validate.rs`, `rules/evaluator.rs` | R-VAL-06, R-EVAL-05, BUG-017, UI-RB-05 |
| FR-055 versioned JSON envelopes (`"v": 1`) | `encode_conditions_json`, `encode_actions_json` | R-RULE-01..02 |
| FR-056 visual rule builder | `RuleBuilderPage.tsx`, `ConditionGroup.tsx`, `ConditionRow.tsx`, `ActionRow.tsx` | UI-RB-01..05 |
| FR-057 live dry-run "test this rule" on folder | `RuleService::test_draft_on_folder`, `RuleBuilderPage.tsx` | UI-RB-06, BUG-016 |
| FR-070–076 dynamic path & naming templates | `rules/templates.rs`, `TemplateInput.tsx` | R-TPL-01..08 + proptest, UI-RB-04/06 |
| FR-005 / FR-120–124 starter rule presets (7 templates, disabled by default) | `rules/presets.rs`, `RulesPage.tsx` | R-TMPL-01..03, UI-RULES-01 |
| Exit: Spec §6.2 invoice rule evaluates end-to-end | `rules/evaluator.rs` | R-EVAL-06 |

## 3. Checklist results

### A. Data safety
- [x] **Zero filesystem mutations in Phase 3.** `rules/conditions.rs`, `rules/evaluator.rs`, `rules/templates.rs`, `rules/validate.rs`, and `rules/presets.rs` are 100% pure functions over `FileFacts` and `Clock`. `RuleService::test_draft_on_folder` reads `files` from SQLite and evaluates rules in memory without touching disk (NFR-004).
- [x] `resolve_destination_path` validates every rendered path through `PathAuthority` against approved roots and rejects `..`, `.`, colons, NUL bytes, components > 255 bytes, and paths > 1024 bytes (`R-TPL-05..07` + 10k proptest).
- [x] `FORBIDDEN_EXECUTABLE_EXTENSIONS` blocks changing extensions to `.app`, `.sh`, `.command`, `.pkg`, `.dmg`, `.exe`, etc. both at save time (`validate.rs`) and at evaluation time (`evaluator.rs`).
- [x] Destination loop guard (`FR-054`): flagged at save time (`validate_rule_draft`) and guarded at runtime (`DestinationLoopGuarded` when destination equals current parent directory).

### B. Correctness
- [x] Unicode NFC normalization applied across text conditions, glob patterns, extensions, locations, source folder scoping, and template variable sanitization (`R-COND-02`, `R-TPL-04`).
- [x] Decimal storage units (`1 KB = 1000 B`, `1 MB = 1_000_000 B`, `1 GB = 1_000_000_000 B`) enforced in `SizeUnit::to_bytes` and shown in the Rule Builder UI (`FR-044`).
- [x] Local timezone & DST boundaries: `Clock` trait (`SystemClock` + `FixedClock`) evaluates `today`, `yesterday`, `this_week`, `this_month`, `this_year`, `before`, `after`, `within_range`, and `older/newer_than_days` in local time and across spring-forward / fall-back transitions without `unsafe` environment mutation (`R-COND-06..10`).
- [x] Compound extensions (`tar.gz`, `tar.bz2`, `tar.xz`, `tar.zst`) matched accurately by `ExtensionGroup` (`R-COND-15`).
- [x] Naming templates (`{date}_{original_name}`) preserve the file extension without doubling `.pdf.pdf` (`R-TPL-08`).

### C. Concurrency & resources
- [x] Regex compilation is bounded by `MAX_REGEX_PATTERN_LEN = 512` and `REGEX_SIZE_LIMIT = 256 KiB` (NFA + DFA); linear-time `regex` crate prevents catastrophic backtracking (`R-COND-03`, `R-VAL-01`).
- [x] Condition nesting bounded to `MAX_CONDITION_DEPTH = 5` both in `validate_rule_draft` and `evaluate_node` (`R-GRP-04`).
- [x] Rule evaluation bounded to `MAX_MOVES_PER_PASS = 1`, `MAX_ACTIONS_PER_PASS = 10`, and `MAX_RULE_CHAIN_DEPTH = 5` with cycle detection (`R-EVAL-03..04`).
- [x] `repo_rules::reorder` runs inside a single SQLite transaction (`BEGIN` … `COMMIT`).
- [x] Rust test suite run 10× consecutively with 0 failures.

### D. Security
- [x] Webview capabilities unchanged; destination folder selection (`rules_pick_destination_folder`) runs the native dialog in Rust and validates the chosen path through `validate_new_root(&path, &self.policy)` before persisting in `destination_roots`.
- [x] No shell/script/command action exists (`FR-051`); deserializing `shell`/`run_script`/`exec` fails (`R-RULE-03`).
- [x] `cargo audit` passes with 0 new dependencies in `Cargo.lock` (ADR-015).

### E. UI / theme
- [x] `pnpm check:tokens` passes; all new components (`RulesPage`, `RuleBuilderPage`, `ConditionGroup`, `ConditionRow`, `ActionRow`, `TemplateInput`) use theme tokens from `src/styles/theme.css` exclusively.
- [x] Empty, loading, recoverable error (+ retry), and inline validation error/warning states implemented on both `/rules` and `/rules/new` (`/rules/:id`).
- [x] Accessible controls: switches labelled via `<label htmlFor>`, icon buttons have `aria-label`, destructive rule deletion uses `ConfirmDialog` stating existing organized files are untouched.

### F. Docs
- [x] `PLAN_TRACKER.md`, `ARCHITECTURE.md`, `TESTING_STRATEGY.md`, `DECISIONS.md` (ADR-015), `BUGS.md` (BUG-014..017), `CHANGELOG.md`, `README.md`, and `audits/phase-3.md` updated and in sync.

## 4. Bugs found and fixed during audit

| BUG ID | Sev | Summary | Regression test | Status |
|---|---|---|---|---|
| BUG-014 | S1 | `RuleService::duplicate` truncated `copy_name` at byte index 120 (`String::truncate(120)`), which panicked on multi-byte UTF-8 rule names when byte 120 fell mid-codepoint and cut off `" Copy"`; `validate_rule_draft` also checked byte length instead of char count. | `rules::service::tests::bug_014_duplicate_multibyte_utf8_name_does_not_panic_and_keeps_copy_suffix` | ✅ Fixed |
| BUG-015 | S1 | `Action::AddPrefix` and `Action::AddSuffix` interpolated the raw affix into a `{original_name}` template string; literal `{` or `}` characters in a prefix/suffix passed `validate_rule_draft` at save time but failed at runtime with `TemplateFailed`. | `rules::evaluator::tests::bug_015_add_prefix_and_suffix_allow_literal_braces_without_template_error` | ✅ Fixed |
| BUG-016 | S1 | `validate_rule_draft` only checked direct self-reference (`A → A`) on `Action::ApplyAnotherRule`, missing multi-hop cycles (`A → B → A`) at save time; and `RuleService::test_draft_on_folder` omitted stored rules from the evaluation pool so `ApplyAnotherRule` targets were not resolved during dry-run folder tests. | `rules::service::tests::bug_016_multihop_apply_another_rule_cycle_rejected_at_save_and_resolved_in_folder_test` | ✅ Fixed |
| BUG-017 | S2 | `has_subdirectory_guard` treated `NOT(InsideSubdirectory)` inside an `ANY` (`OR`) branch as a loop guard for recursive monitored folders, suppressing the `destination_loop` warning even when other `ANY` branches still matched files in the destination subfolder. | `rules::validate::tests::bug_017_not_inside_subdirectory_in_any_branch_does_not_suppress_destination_loop_warning` | ✅ Fixed |

## 5. Process record

- Red-first confirmed for every Phase 3 task:
  - `P3-T01` (`rules/models.rs`): 3 tests (`r_rule_01..03`) failed on stub `todo!()` before implementation.
  - `P3-T02` & `P3-T03` (`rules/conditions.rs`): 21 tests (`r_cond_01..15`, `r_grp_01..05`, 2 proptests) failed before implementation.
  - `P3-T04` (`rules/validate.rs`): 6 tests (`r_val_01..06`) failed before implementation.
  - `P3-T05` (`rules/templates.rs`): 9 tests (`r_tpl_01..08` + proptest) failed before implementation.
  - `P3-T06` (`rules/evaluator.rs`): 6 tests (`r_eval_01..06`) failed before implementation.
  - `P3-T07` (`rules/service.rs`, `repo_rules.rs`): 3 tests (`r_rule_04..06`) failed before implementation.
  - `P3-T08` (`rules/presets.rs`): 3 tests (`r_tmpl_01..03`) failed before implementation.
  - `P3-T09` & `P3-T10` (`src/pages/rules/rules.test.tsx`): 12 UI tests (`UI-RULES-01..03`, `UI-RB-01..06`) failed against stub pages before implementation.
  - `BUG-014..017`: all 4 regression tests written and verified failing before applying fixes.

## 6. Deferred / owner actions

| Item | Reason | Target |
|---|---|---|
| Visual QA of Rules & Rule Builder screens in `pnpm tauri dev` (light/dark/compact) and testing native destination folder picker | Native dialog cannot be clicked in headless test runner | Owner, now |
| Full multi-rule preview diff screen (`/preview`) & conflict simulation | Planned | Phase 4 (`P4-T01..T07`) |
| Safe filesystem execution (`safe_fs` + write-ahead ledger + undo) | Planned | Phase 5 (`P5-T01..T10`) |
| Rule JSON import/export UI | Planned | Phase 7 (`P7-T05`) |

## 7. Sign-off

- [x] All S0/S1 bugs fixed (`BUG-014`, `BUG-015`, `BUG-016`, `BUG-017`)
- [x] All gates green after fixes; 10k proptests green; Rust suite 10/10 consecutive runs green
- [x] Tracker and docs updated

