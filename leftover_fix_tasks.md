# Leftover Carry-Forward Fix Tasks

## Goal

Fix carry-forward so `closing_leftover` from one report is correctly reflected in subsequent report `opening_leftover`, and make file ordering use true covered period end.

## Scope

- In: chronology validation, `file_history` period metadata, latest report selection, regression tests.
- Out: leftover math changes, borrowed scaffolding behavior changes, report layout changes.

## Action Items

- [x] Add failing integration test: overlapping second file must be rejected.
- [x] Add failing carry-forward regression test: later second file should preserve opening/closing continuity for same product+department.
- [x] Add repository query for latest ledger transaction date and use it in chronology validation.
- [x] Add schema migration to extend `file_history` with `period_end` and backfill from ledger (`MAX(transaction_date)` per `file_hash`).
- [x] Extend `FileHistory` model and repository read/write paths to persist/read `period_end`.
- [x] Persist `period_end` during ingestion commit from pending period max date.
- [x] Update latest file selection for report regeneration to order by `period_end`.
- [x] Add migration coverage tests for `period_end` creation and backfill.
- [x] Verify borrowed scaffolding behavior remains unchanged.
- [x] Run `cargo fmt`, `cargo check --locked`, and `cargo test --locked`.
