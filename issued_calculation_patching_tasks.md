# Issued Calculation Patching Tasks

## Overview

Add "จ่าย" (issued) calculation to report with formula:
```
จ่าย = floor((เบิก - (carry_over_borrowed + ingested_borrowed)) / factor)
```

- `carry_over_borrowed`: persisted silently in database (subunits), carries over indefinitely until consumed
- `ingested_borrowed`: from Excel file (future feature, default 0 for now)
- Result is integer whole units, can be negative
- Report displays only `issued` integer, preserving current format

---

## Tasks

### Database Migration

- [x] Add migration v3: `borrowed_amount TEXT DEFAULT '0'` to `inventory_ledger` table
- [x] Add migration v3: create `borrowed_carryover(product_id, department_id, amount, updated_at)` table

### Model Updates

- [x] Add `borrowed_amount: Decimal` to `LedgerEntry` (default ZERO) in src/models.rs
- [x] Add `borrowed: Decimal`, `issued: Decimal` to `DryRunRow` in src/domain.rs

### Repository Layer

- [x] Add `get_borrowed_carryover(product_id, department_id)` to src/repository.rs
- [x] Add `upsert_borrowed_carryover(product_id, department_id, amount)` to src/repository.rs
- [x] Update `batch_insert_ledger()` to persist `borrowed_amount` column
- [x] Update `get_ledger_entries_by_file_hash()` to read `borrowed_amount`

### Business Logic

- [x] Update `build_dry_run_rows()` in src/ingestion.rs:
  - Fetch carry_over_borrowed per (product, department)
  - Calculate `issued = floor((dispensed - carry_over - ingested_borrowed) / factor)`
  - Update carry-over: if `net_subunits < 0`, persist `abs(net_subunits)`; else clear
- [x] Update `build_report_rows_for_entries()` in src/report.rs with same logic
- [x] Ensure carry-over persistence happens after report calculation

### Report Rendering

- [x] Update `ReportTemplateDepartmentRow`: add `issued_val: Decimal`, keep `issued: String`
- [x] Update `render_report_html()`: set `issued = decimal_to_string(issued_val)`
- [x] Update report.html.tera: change `issued` column to display calculated value (remove blank-value class)
- [x] Update summary section: show total issued (sum of department issued values)

### Testing

- [x] Add unit test: `issued = floor((5-1)/2) = 2` with factor=2
- [x] Add unit test: `issued = floor((3-7)/2) = -2`, carryover persists as 4
- [x] Add unit test: carryover consumed: `floor((10-4)/2) = 3`, carryover cleared
- [x] Run `cargo test` to validate all changes

### Version Update

- [x] Update app version to v0.2.2 in Cargo.toml and any version constants

---

## Data Flow Example

| File | dispensed | carry_over_borrowed | net_subunits | factor | issued | new_carryover |
|------|-----------|---------------------|--------------|--------|--------|---------------|
| 1    | 3         | 0                   | 3            | 2      | 1      | 0             |
| 2    | 3         | 7                   | -4           | 2      | -2     | 4             |
| 3    | 10        | 4                   | 6            | 2      | 3      | 0             |

---

## Report Column Changes

**Before:**
```html
<td class="num">{{ dept.issued }}</td>  <!-- blank -->
<div class="summary-item"><span>จ่าย</span><strong class="blank-value">&nbsp;</strong></div>
```

**After:**
```html
<td class="num">{{ dept.issued }}</td>  <!-- integer, may be negative -->
<div class="summary-item"><span>จ่าย (รวม)</span><strong>{{ row.total_issued }}</strong></div>
```
