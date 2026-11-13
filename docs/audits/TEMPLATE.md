# Phase N Audit — <name>

| Field | Value |
|---|---|
| Phase | N |
| Audit date | YYYY-MM-DD |
| Status | 🔍 In audit / ✅ Signed off |
| Commit | `<sha>` |

## 1. Gate results

| Gate | Result |
|---|---|
| cargo fmt --check | |
| cargo clippy -D warnings | |
| cargo test | passed / total |
| cargo audit | |
| pnpm lint / typecheck / test / build | |
| pnpm check:tokens | |
| Previous phases' tests | |

## 2. FR coverage

| FR | Test IDs | Pass |
|---|---|---|

## 3. Checklist results

Copy relevant sections (A–F) from AUDIT_PROCESS.md and tick them.

## 4. Bugs found

| BUG ID | Sev | Summary | Regression test | Status |
|---|---|---|---|---|

## 5. Deferred items

| Item | Sev | Reason | Target phase |
|---|---|---|---|

## 6. Docs drift fixed

-

## 7. Sign-off

- [ ] All S0/S1 fixed
- [ ] All gates green after fixes
- [ ] Tracker updated
