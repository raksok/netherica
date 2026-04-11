# Report Fix Tasks

## Goal

Fix report rendering so department-level leftovers are visible and the dispensed column header shows product subunit.

## Planned Changes

- [x] Add `closing_leftover` to report department row template data in `src/report.rs`.
- [x] Add `subunit` to report product metadata and template row context in `src/report.rs`.
- [x] Populate `subunit` in metadata builders in `src/ingestion.rs` and `src/report.rs`.
- [x] Update report template table header to add `ยอดยกไป` before `unit` in `asset/templates/report.html.tera`.
- [x] Render department-level closing leftover values in the new `ยอดยกไป` column.
- [x] Change `เบิก` header to two lines with subunit: `เบิก` and `({subunit})`.
- [x] Rebalance table widths/CSS so the extra column and two-line header still fit A4 landscape output.
- [x] Update/add report rendering tests in `src/report.rs` for the new header and column behavior.
- [x] Run validation: `cargo check --locked` and `cargo test --locked`.
