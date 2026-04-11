# Netherica (Rust) – v3.2

## Production Architecture & Requirements Specification

This document merges v2.5 (user‑facing requirements) and v3.0 (production architecture) with resolved conflicts based on the following design decisions:

| Decision | Choice |
|----------|--------|
| `file_history.status` | **No status column** – presence implies committed |
| Chronological order | **Always strict** – reject files older than latest ledger entry |
| `product_totals` | **Keep, incremental update** (add new amounts to existing total) |
| Path resolution | **Executable dir first, fallback to OS user data dir** |
| Duplicate file hash | **Strict reject** – no override |
| Dry run table | **Keep detailed v2.5 layout** – required for user confirmation |\r
| GUI design system | **Nordic Precision** – Stitch MCP project `netherica design`; see `DESIGN.md` |

---

## 1. Overview

### 1.1 Purpose

A high‑reliability, portable desktop application built in **Rust** to automate pharmacy transaction reconciliation using an **event‑sourced model**.

**Priorities:**
- Deterministic inventory computation
- Auditability (append‑only ledger)
- Portability (single binary)
- Fault tolerance under real‑world usage (cloud sync, user errors, partial failures)

### 1.2 Key Design Principles

1. **Event Sourcing First** – No mutable inventory state; all results derived from immutable ledger.
2. **Idempotent Ingestion** – Same file cannot be processed twice.
3. **Strict Chronological Order** – Files must be processed in increasing transaction date order.
4. **Fail‑Safe Operations** – Partial failures never corrupt system state.
5. **Portable but Resilient** – Works from USB/cloud folders with fallback paths.
6. **Internal Gregorian, Display Buddhist** – All dates stored/processed as Gregorian ISO 8601; reports display Buddhist Era (BE) years.

---

## 2. System Architecture

### 2.1 High‑Level Components

```
+---------------------+
|        GUI          |
| (egui / eframe)    |
+----------+----------+
           |
           v
+---------------------+
| Application Layer   |
| - Workflow Engine   |
| - Validation        |
| - Report Builder    |
+----------+----------+
           |
           v
+---------------------+
| Domain Layer        |
| - Ledger Logic      |
| - Aggregation       |
| - Business Rules    |
+----------+----------+
           |
           v
+---------------------+
| Infrastructure      |
| - SQLite (WAL)      |
| - File System       |
| - Excel Parser      |
| - Logging           |
+---------------------+
```

### 2.2 Module Breakdown (Rust)

```
src/
 ├── main.rs
 ├── app/
 │    ├── mod.rs
 │    ├── state.rs      # GUI state machine (Idle, DryRun, Complete)
 │    ├── workflow.rs   # Background worker coordination
 │    └── messages.rs   # mpsc messages (Progress, Completed, Error)
 │
 ├── domain/
 │    ├── ledger.rs      # Event sourcing queries
 │    ├── product.rs     # Product config & factor handling
 │    ├── aggregation.rs # Department grouping, totals
 │    └── math.rs        # Euclidean modulo, Decimal arithmetic
 │
 ├── infrastructure/
 │    ├── db.rs          # SQLite connection & WAL setup
 │    ├── repository.rs  # CRUD for file_history, ledger, product_totals
 │    ├── excel.rs       # calamine parser, header mapping
 │    ├── filesystem.rs  # File picker, archive move, path resolution
 │    └── logging.rs     # Structured logging with rotation
 │
 ├── reporting/
 │    ├── generator.rs   # tera HTML generation, font embedding
 │    └── templates/     # HTML template with print CSS
 │
 └── config/
      ├── loader.rs      # config.toml parsing & default generation
      └── schema.rs      # Rust structs for config
```

---

## 3. Configuration (`config.toml`)

If missing at startup, the application generates a default template with empty `[[products]]` and `[departments]`.

```toml
[settings]
strict_chronological = true   # Reject files older than latest ledger entry

[departments]
"ER_CODE" = "Emergency Room"
"OPD_A"   = "Outpatient Ward"

[[products]]
id            = "GAUZE-01"      # Must match Excel sheet name exactly
display_name  = "Gauze 500cm"
unit          = "Roll"
subunit       = "cm"
factor        = "500"           # String → Decimal for exact precision
track_subunits = true
```

**Validation (fail-fast at startup):**
- `factor` must be > 0 and parseable as `Decimal`.
- If `track_subunits = false`, `factor` must be `"1"`.
- At least one `[[products]]` entry required.
- All `product.id` values must be unique (reject duplicates).
- At least one `[departments]` entry required.
- Each `product.id` should match an Excel sheet name (warn on startup if missing).

---

## 4. Data Architecture

### 4.1 Database Engine

- **SQLite** via `rusqlite` (bundled feature)
- **WAL mode** enabled: `PRAGMA journal_mode = WAL;`
- **Synchronous = NORMAL**: `PRAGMA synchronous = NORMAL;`
- Connection instantiated with these pragmas on every open.

### 4.2 Schema

#### `file_history` (no status column – presence implies committed)

```sql
CREATE TABLE file_history (
    file_hash TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    transaction_date DATETIME NOT NULL,   -- Gregorian ISO 8601 (converted from BE)
    processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

#### `inventory_ledger` (append‑only)

```sql
CREATE TABLE inventory_ledger (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_hash TEXT NOT NULL,
    product_id TEXT NOT NULL,
    department_id TEXT NOT NULL,          -- raw code, e.g., "ER_CODE"
    dispensed_amount TEXT NOT NULL,       -- Decimal as string, negative allowed
    transaction_date DATETIME NOT NULL,   -- Gregorian ISO 8601 (converted from BE)
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
);
```

#### `product_totals` (optimisation layer – incremental update)

```sql
CREATE TABLE product_totals (
    product_id TEXT NOT NULL,
    department_id TEXT NOT NULL,          -- raw code, e.g., "ER_CODE"
    total_sum TEXT NOT NULL,               -- running sum of dispensed_amount
    PRIMARY KEY (product_id, department_id)
);
```

**Update rule (incremental):**
```sql
INSERT INTO product_totals(product_id, department_id, total_sum)
VALUES (?, ?, ?)
ON CONFLICT(product_id, department_id) DO UPDATE SET total_sum = total_sum + excluded.total_sum;
```

**Note:** `product_totals` reflects the **cumulative sum of all transactions per (`product_id`, `department_id`) regardless of order**. For per‑file opening leftover calculations (e.g., dry run), the system must use a date‑filtered query on `inventory_ledger`, not `product_totals`.

### 4.3 Schema Migrations

- Schema version stored in `PRAGMA user_version`.
- On startup, check `user_version` and apply incremental migrations.
- Each migration is a separate SQL file compiled into binary via `rust-embed`.
- Migration list:
  - `v0 → v1`: Initial schema creation (tables, indexes).
  - `v1 → v2`: Upgrade `product_totals` key from legacy single key to composite key (`product_id`, `department_id`).
    - Backfill source: recompute from `inventory_ledger` via `GROUP BY product_id, department_id` and `SUM(dispensed_amount)`.
    - Index update: remove obsolete unique index/constraint on legacy `product_totals(product_id)`; enforce composite uniqueness with `PRIMARY KEY (product_id, department_id)` (or equivalent unique composite index).
    - Transactional execution: run `CREATE TABLE product_totals_v2 ...`; backfill `INSERT ... SELECT ... GROUP BY ...`; validate row counts/sums; swap tables (`DROP` old, `ALTER TABLE ... RENAME TO product_totals`); recreate dependent indexes; set `PRAGMA user_version = 2`; commit atomically.
- Migration runs inside transaction; on failure, abort startup with clear error.

#### Indexes

```sql
CREATE INDEX idx_ledger_product_date ON inventory_ledger(product_id, transaction_date);
CREATE INDEX idx_ledger_file_hash ON inventory_ledger(file_hash);
```

---

## 5. Ingestion Pipeline

### 5.1 Workflow States (GUI)

1. **Idle** – Waiting for file selection.
2. **Parsing** – Background thread reads Excel, computes dry run.
3. **Dry Run (Review)** – User sees table; can Cancel or Confirm.
4. **Committing** – ACID transaction in progress.
5. **Complete** – Report generated, file archived.

### 5.2 Ingestion Flow (Detailed)

```rust
// 1. File selected via rfd
// 2. Compute SHA‑256 hash -> check file_history. Reject if exists (duplicate).
// 3. Extract transaction_date from Excel:
//    - Per-row from column 5 ('Date Visit'), format DD-MM-YYYY HH:MM
//    - Use earliest date in file as file's transaction_date
//    - Fallback: Use file modification time (UTC), log warning, show toast to user
//    - User must acknowledge fallback before confirming
// 4. Query MAX(transaction_date) from file_history. If selected date <= existing max,
//    abort with error: "File date is older or equal to the last processed file. Please process in chronological order."
// 5. Parse Excel sheets matching [[products]].id == sheet_name -> build ledger rows:
//    - Validate column 13 ('Code') matches sheet name
//    - Extract: date (col 5), department (col 10), product_id (col 13), qty (col 15)
// 6. Compute dry run data (see Section 7) and display to user.
// 7. User confirms -> begin ACID commit:

BEGIN TRANSACTION;

INSERT INTO file_history (file_hash, filename, file_size, transaction_date)
VALUES (?, ?, ?, ?);

INSERT INTO inventory_ledger (file_hash, product_id, department_id, dispensed_amount, transaction_date)
VALUES (?, ?, ?, ?, ?);   -- one per Excel row

-- Update product_totals incrementally (one INSERT OR UPDATE per product per department)
INSERT INTO product_totals (product_id, department_id, total_sum)
VALUES (?, ?, ?)
ON CONFLICT(product_id, department_id) DO UPDATE SET total_sum = total_sum + excluded.total_sum;

COMMIT;

-- After commit:
// 8. Generate HTML report (using ledger data from this file + previous totals).
// 9. Write HTML to ./reports/YYYYMMDD_HHMMSS_report.html.
// 10. Move Excel file to ./archive/YYYYMMDD_HHMMSS_filename.xlsx.
// 11. If step 8,9,10 fail: log error, but DB remains consistent; user can retry report generation.
```

### 5.3 Idempotency & Duplicate Handling

| Condition | Action |
|-----------|--------|
| Same `file_hash` | **Reject** – “This file has already been processed.” |
| Same transaction date, different hash | Allow (different exports on same day). |
| Same filename, different hash | Allow (file content changed). |

### 5.4 Chronological Order Enforcement (Strict)

- **Reject** if `selected_file.transaction_date <= max(file_history.transaction_date)`.
- Error message: *“File date is older or equal to the last processed file. Please process in chronological order.”*
- No override option in v3.1.

---

## 6. Domain Logic

### 6.1 Standard Error Types

```rust
pub type AppResult<T> = Result<T, AppError>;

const BUDDHIST_ERA_OFFSET: i32 = 543;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("File I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Excel parsing error: {0}")]
    Excel(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Duplicate file: {0}")]
    DuplicateFile(String),
    #[error("Chronological violation: new date {new} <= existing max {existing}")]
    ChronologicalViolation { new: DateTime, existing: DateTime },
}
```

All domain functions return `AppResult<T>`. No panics in production code.

### 6.2 Inventory Model (Event Sourced)

For a given (`product_id`, `department_id`) and cutoff date:

```rust
let total_sum = db.sum_ledger_for_product_department_before_date(product_id, department_id, cutoff_date);
let factor = Decimal::from_str(&config.factor).unwrap();
let leftover = euclidean_mod(total_sum, factor);
let whole_units = (total_sum / factor).floor();
```

### 6.3 Mathematical Guarantees

- **Euclidean modulo** for leftovers:
  `euclidean_mod(a, n) = ((a % n) + n) % n` where `n > 0`.
- **Decimal arithmetic** via `rust_decimal` (no floating point).
- **`factor == 1` rule:** leftover tracking is disabled; opening leftover and closing leftover are always treated as `0`.
- **Whole units consumed this period** =
  `(new_total_sum / factor).floor() - (prior_total_sum / factor).floor()` where both totals are scoped to the same (`product_id`, `department_id`).

### 6.4 Aggregation Strategy

- **`product_totals`** – O(1) access to current cumulative sum per (`product_id`, `department_id`) (used for “current leftover” display, e.g., after processing).
- **Date‑filtered ledger** – Used for dry run (opening leftover as of file’s transaction date) and for historical reports.
- **Fallback** – If `product_totals` is missing (e.g., first run), recompute from ledger.

---

## 7. Graphical User Interface (GUI)

> **Design Source:** Stitch MCP project `netherica design` (ID `18330809547273064391`)
> **Design System:** Nordic Precision — Creative North Star: _"The Arctic Atelier"_
> **Full Specification:** See `DESIGN.md` for complete design tokens, color palette, typography scale, and component guidelines.
> **Last Synced:** 2026-04-11 · **Canonical Screens:** 7 (Idle, Parsing, Review, Complete, Departments, Products, + reference upload)

### 7.1 Design System Summary

The Netherica GUI follows the **"Editorial Calm"** design philosophy: a high-end editorial space prioritizing atmospheric depth over structural rigidity. The interface uses **egui/eframe** with custom theming mapped to the Nordic Precision token set.

#### Core Design Principles

| Principle | Implementation |
|---|---|
| **Dark Mode** | Base canvas `#0d131e`, never pure black `#000000` |
| **No-Line Rule** | 1px borders prohibited for sectioning; use background-color shifts only |
| **Tonal Layering** | Elevation via surface token progression, not drop shadows |
| **Glass & Gradient** | Primary CTAs use 135-degree gradient (`#a3dcec` to `#88c0d0`) |
| **Breathing Room** | Increase spacing when crowded -- never add divider lines |

#### Surface Hierarchy (egui `Visuals` mapping)

| Layer | Token | Hex | Usage |
|---|---|---|---|
| Deepest | `surface-container-lowest` | `#080e19` | Input field fills |
| Base | `surface` | `#0d131e` | Window background canvas |
| Secondary | `surface-container-low` | `#161c27` | Sidebar, secondary panels |
| Content | `surface-container` | `#1a202b` | Primary workspace, cards |
| Interactive | `surface-container-high` | `#242a36` | Hover states, active elements |
| Overlay | `surface-container-highest` | `#2f3541` | Modals, inactive wizard |

#### Typography

| Slot | Font | Size | Notes |
|---|---|---|---|
| Headlines | Inter | 2rem (32px), -0.02em tracking | Editorial authority |
| Body | Inter | 0.875rem (14px) | Primary content |
| Labels | Inter | 0.75rem (12px), ALL CAPS, +0.05em | Table headers, wizard steps |
| Thai Fallback | Sarabun (THSarabunNew.ttf) | Matches body sizing | Embedded via `rust-embed` |

#### Core Color Tokens

| Token | Hex | Role |
|---|---|---|
| `primary` | `#a3dcec` | Active elements, links, focused labels |
| `primary-container` | `#88c0d0` | Gradient endpoint, contained primary |
| `secondary` | `#a9caeb` | Secondary interactive elements |
| `tertiary` | `#f0cf8f` | Warning indicators (4px dots only) |
| `error` | `#ffb4ab` | Error indicators (4px dots only, never background fills) |
| `on-surface` | `#dde2f2` | Primary text |
| `on-surface-variant` | `#c0c8cb` | Secondary text, descriptions |
| `outline-variant` | `#40484b` | Ghost borders at 15% opacity |

### 7.2 Application Layout

The application uses a **persistent sidebar + central content area** layout:

```
+--------------------------------------------------+
|  +----------+  +-------------------------------+ |
|  |          |  |                               | |
|  |  Sidebar |  |       Central Content         | |
|  |          |  |                               | |
|  |  - Logo  |  |   (View-specific content      | |
|  |  - Nav   |  |    determined by current       | |
|  |    items |  |    navigation + workflow       | |
|  |          |  |    state)                      | |
|  |          |  |                               | |
|  |          |  |                               | |
|  |          |  |                               | |
|  +----------+  +-------------------------------+ |
|  Footer: "Precision Reconciliation Engine"       |
+--------------------------------------------------+
```

#### Sidebar (Persistent)

Background: `surface-container-low` (`#161c27`)

| Icon | Label | Section | Description |
|---|---|---|---|
| `upload_file` | **Ingestion** | Primary workflow | File upload, parsing, review, commit |
| `analytics` | **Reports** | Report browser | Generated report access (future scope) |
| `settings` | **Settings** | Configuration | Products, Departments, App settings |

- **Active item:** `primary` (`#a3dcec`) text color + `surface-container` background highlight
- **Inactive items:** `on-surface-variant` (`#c0c8cb`) text
- Logo: "Netherica" headline + "Pharmacy Reconciliation" subtitle

### 7.3 Ingestion View States

The Ingestion section uses a linear workflow state machine:

```
+-------+     +---------+     +--------+     +------------+     +----------+
| Idle  |---->| Parsing |---->| Review |---->| Committing |---->| Complete |
+-------+     +---------+     |(DryRun)|     +------------+     +----------+
   ^                          +---+----+                            |
   |                              | Cancel                          |
   |                              v                                 |
   |                           +------+                             |
   +---------------------------| Idle |<----------------------------+
                               +------+         "New File"
```

#### 7.3.1 Idle View

- **Headline:** "Ready for new reconciliation run?"
- **Description:** "Upload your latest Pharmacy Excel export to begin the automated discrepancy audit."
- **Primary Action:** File upload drop zone / button -- triggers `rfd` native file picker
- **Status Cards** (3-column grid, `surface-container-low` cards):
  - **Last Run:** Timestamp of most recent `file_history` entry (or "No files processed" if empty)
  - **Sync State:** Database connection status: Connected / Fallback path used
  - **Config Status:** Product/department count summary from `config.toml`

#### 7.3.2 Parsing View

- **Headline:** "Analyzing Data Structure"
- **Sub-headline:** "Parsing sheet: {current_sheet_name}..."
- **File Metadata Card** (`surface-container` background):
  - Filename, file size, detected sheet count
  - List of matched sheet names
- **Live Log Output** (terminal-style, `surface-container-lowest` background):
  - Scrolling log prefixed with timestamps: `[HH:MM:SS] LEVEL: message`
  - Examples: `[14:22:01] INFO: Initializing stream reader for 'GAUZE-01'...`
  - Warnings highlighted with `tertiary` (`#f0cf8f`) text
- **Progress Bar:**
  - Track: `surface-container-highest`
  - Fill: Gradient `primary` to `secondary` (`#a3dcec` to `#a9caeb`)
  - Label: Current row / total rows per sheet
- **No user-interruptible actions** during parsing (background thread via `mpsc`)

#### 7.3.3 Review View (Dry Run)

- **Headline:** "Dry Run Review"
- **Description:** "Validate reconciliation metrics before generating the final ledger report."
- **Summary Metric Cards** (3-column, `surface-container-low`):
  - **Accuracy Score:** Variance threshold indicator
  - **Inventory Warnings:** Count of products with potential stock-out or unusual deltas
  - **Computation Time:** Elapsed time for dry run calculation
- **Reconciliation Ledger Table** (`egui_extras::TableBuilder`):

| Product | Department | Opening Leftover (subunits) | Total Subunits Used | Whole Units Output | Closing Leftover (subunits) |
|---------|------------|-----------------------------|---------------------|--------------------|----------------------------|

  - **Table styling:**
    - Header: `surface-container-low` background, `label-sm` uppercase text
    - Rows: No grid lines, `1.5rem` row height, hover to `surface-container-high`
    - Ghost borders at 15% opacity for column separation if needed
  - **Department:** The department code/name for this row.
  - **Opening Leftover:** Computed from ledger for the same (`product_id`, `department_id`) before this file's transaction date; if `factor == 1`, this value is always `0`
  - **Total Subunits Used:** Explicitly per one `Product + Department` pair (not product-wide)
  - **Whole Units Output:** As defined in Section 6.2
  - **Closing Leftover:** Opening + total subunits used, modulo factor; if `factor == 1`, this value is always `0`
  - **Row count indicator:** "Showing {visible} of {total} adjustment rows (Product + Department)"
- **Status Indicators:** 4px dots -- `error` for failures, `tertiary` for warnings
- **Confirmation Footer:**
  - Warning text: "Confirming will finalize the period-end reconciliation and update the central inventory ledger."
  - Buttons: `[Cancel]` (secondary, back to Idle) and `[Confirm & Generate Report]` (primary, gradient)

#### 7.3.4 Completion View

- **Headline:** "Process Completion"
- **Success Banner:** "Reconciliation Successful" with checkmark icon
  - Description: "Report successfully generated for {filename}. The pharmaceutical assets have been synchronized with the master ledger."
- **Summary Metrics** (inline):
  - **Rows Processed:** Total ledger entries created
  - **Discrepancies Resolved:** Count (usually 0 for clean runs)
- **Action Cards** (2-column, `surface-container-low`):
  - **"Open Report Folder"** -- Opens `./reports/` directory in system file manager
  - **"Regenerate Last Report"** -- Re-queries ledger and recreates HTML
- **New Cycle CTA:** "Begin a new ingestion cycle for the next inventory manifest or clinical batch file." -- Button returns to Idle
- **System Health & Metadata** (footer section, `label-md` uppercase):
  - Execution Time | Data Integrity % | Validator Version | Log Hash
- **Footer:** "Precision Reconciliation Engine"

### 7.4 Settings Views

The Settings section contains two sub-views accessible via tabs or sub-navigation.

#### 7.4.1 Settings: Departments

- **Headline:** "Department Configuration"
- **Section Title:** "Department Registry"
- **Description:** "Manage departmental taxonomy and display mapping."
- **Department List** (card-based grid, `surface-container-low` cards with `radius-xl`):
  - Each card displays:
    - **Department Code** (monospace, `label-md` uppercase)
    - **Mapped Display Name** (headline-sm)
  - Hover: Background shifts to `surface-container-highest`, ghost border at 30%
- **Add Department:**
  - Section: "Provision New Node" / "Register Department"
  - Input fields: Department Code + Mapped Display Name
  - Uses `surface-container-lowest` fills, no borders, focus ghost border at 40%
  - Validation: Code must be unique, non-empty
- **Relationship to `config.toml`:** Edits here update the `[departments]` section

#### 7.4.2 Settings: Products

- **Headline:** "Product Configuration"
- **Section Title:** "Inventory Ledger"
- **Description:** "Configure medical supplies, reconciliation factors, and tracking units."
- **Product List** (card-based grid, `surface-container-low` cards):
  - Each card displays:
    - **Product ID** (monospace, `label-md`)
    - **Display Name** (headline-sm)
    - Details: Unit, Subunit, Factor, Track Subunits toggle
  - Hover: Background shifts to `surface-container-highest`, ghost border at 30%
- **Add New Entry:**
  - Section: "Create a new product definition for the reconciliation engine."
  - Fields: ID, Display Name, Unit, Subunit, Factor, Track Subunits
  - Validation: ID must be unique, factor > 0, if `track_subunits = false` then `factor = "1"`
- **System Status Footer:**
  - Configuration Integrity percentage
  - Sync status with config file
- **Relationship to `config.toml`:** Edits here update the `[[products]]` entries

### 7.5 Error Handling & Notifications

| Severity | Display Method | Token | Examples |
|---|---|---|---|
| **Critical** | Modal dialog (glassmorphism overlay) | `error` / `error-container` | File lock, duplicate hash, chronological violation |
| **Warning** | Toast notification (auto-dismiss) | `tertiary` | Missing column on one sheet, null date fallback |
| **Info** | Inline status or log entry | `on-surface-variant` | Sheet skipped, row parsed |

- **Toast Notifications:**
  - Position: Top-right overlay
  - Background: `surface-container-highest` with glassmorphism (`backdrop-filter: blur(12px)`)
  - Auto-dismiss after 5 seconds, manually dismissible
- **Modal Dialogs (Critical):**
  - Background overlay: `surface-variant` at 70% opacity with blur
  - Modal card: `surface-container-high` with ambient shadow (`0 24px 48px rgba(0,0,0,0.4)`)
  - Action buttons: Acknowledge / Retry as appropriate
- **All warnings/errors** written to `pharmacy_app.log` (structured, rotated daily, max 5 MB)

### 7.6 Component Quick Reference (egui Mapping)

| Stitch Component | egui Implementation | Key Token |
|---|---|---|
| Primary Button | `egui::Button` with custom `Visuals` | Gradient `#a3dcec` to `#88c0d0` |
| Secondary Button | `egui::Button` with `surface-container-highest` fill | `#2f3541` |
| Ghost Button | `egui::Button` with transparent fill | `primary` text only |
| Input Field | `egui::TextEdit` | `surface-container-lowest` fill |
| Data Table | `egui_extras::TableBuilder` | No grid lines, hover rows |
| Settings Card | `egui::Frame` with `radius-xl` | `surface-container-low` |
| Progress Bar | `egui::ProgressBar` custom painted | Gradient fill |
| Toast | Custom overlay widget | Glassmorphism |
| Modal | Custom overlay with backdrop | Ambient shadow |

---

## 8. Reporting Engine (HTML + Browser Print)

### 8.1 HTML Generation

- **Templating:** `tera` (Jinja2 syntax).
- **Embedded Thai font:**
  - Compile‑time embedding via `rust-embed`.
  - Font file (`Sarabun.ttf` in `assets/`.
  - Injected as `data:font/truetype;base64,` URI inside `<style> @font-face`.
- **Print‑optimised CSS** using `@media print` (hides buttons, sets page breaks, ensures clean margins, landscape orientation).

### 8.2 Report Contents

**All report dates display Buddhist Era (BE) years for user familiarity.**

- **Header:** Processed filename, local timestamp (BE), transaction date range (BE).
- **For each `Product + Department` row (deterministic sort: product then department):**
  - Opening leftover per department row (subunits, scoped by `product_id + department_id`; if `factor == 1`, always `0`)
  - Consumption per department (table rows)
  - Total subunits consumed
  - Whole units output
  - Closing leftover per department row (subunits, scoped by `product_id + department_id`; if `factor == 1`, always `0`)
- **Carry-over rule:** include any `Product + Department` row whose opening leftover is non-zero, even when the current file contains no transaction for that pair.
- **Footer:** Generation timestamp (BE), report version v3.1, file hash.

### 8.3 Year Conversion for Reports

Convert Gregorian (internal) to Buddhist Era (display) using `BUDDHIST_ERA_OFFSET` constant (543):

```rust
fn format_be_date(dt: &DateTime<Utc>) -> String {
    let be_year = dt.year() + BUDDHIST_ERA_OFFSET;
    dt.format(&format!("%d-%m-{} %H:%M", be_year)).to_string()
}
```

**Example output in report:**
- Internal: `2024-03-01T14:30:00Z`
- Display: `01-03-2567 14:30`

### 8.4 Workflow After Commit

1. Generate HTML using ledger data from the committed file (plus prior totals per `product_id + department_id` for opening balances).
2. Write HTML to `./reports/YYYYMMDD_HHMMSS_report.html` (create directory if missing).
3. Open in system default browser using `open` / `start` / `xdg-open`.
4. Show dialog: *“Report ready. Press Ctrl+P to print or save as PDF.”*

### 8.5 Regeneration

- Button **“Regenerate Last Report”** re‑queries ledger for the most recent `file_hash` and recreates the HTML.
- Does not require re‑processing the Excel file.

---

## 9. File Handling & Path Resolution

### 9.1 Path Resolution Strategy (Priority order)

1. **Executable directory** – for `state.db`, `config.toml`, `./archive/`, `./reports/`.
2. If executable directory is **not writable** (e.g., installed in Program Files), fallback to OS user data directory:
   - Windows: `%APPDATA%\Netherica\`
   - Linux: `~/.local/share/Netherica/`
   - Show a one‑time warning: *“Data stored in user profile – not portable.”*

### 9.2 Archive Strategy

- After successful commit, move the original Excel file to `./archive/YYYYMMDD_HHMMSS_filename.xlsx` (local time).
- If the archive directory does not exist, create it.
- If moving fails (e.g., file lock), set a retry flag; the file remains in place and a warning is logged. User can manually retry via a “Retry Archive” button.

---

## 10. Excel File Structure

### 10.1 Column Layout (Fixed Position)

| Column # | Header | Data Type | Description |
|----------|--------|-----------|-------------|
| 5 | `Date Visit` | DateTime string | Format: `DD-MM-YYYY HH:MM` (Thai locale) |
| 10 | `Cosume Department` | String | Cost center / department code |
| 13 | `Code` | String (strict) | Product ID (backup to sheet name) |
| 14 | `Code Name` | String | Product display name (informational) |
| 15 | `Qty` | Decimal | Transaction amount (positive=issue, negative=return) |

### 10.2 Product Identification

**Primary:** Sheet name = Product ID (exact match against `[[products]].id`)

**Secondary:** Column 13 (`Code`) validated against sheet name. Mismatch → reject row, log warning.

```rust
fn validate_product_id(sheet_name: &str, code_column: &str) -> AppResult<&str> {
    if sheet_name != code_column {
        return Err(AppError::Validation(format!(
            "Product ID mismatch: sheet='{}', code='{}'",
            sheet_name, code_column
        )));
    }
    Ok(sheet_name)
}
```

### 10.3 Date Parsing & Year Conversion

**All internal dates use Gregorian calendar with ISO 8601 format (`YYYY-MM-DDTHH:MM:SSZ`).**

Excel dates use Buddhist Era (BE) years. Convert to Gregorian (CE) on ingestion using `BUDDHIST_ERA_OFFSET` constant (543).

```rust
fn parse_transaction_date(date_str: &str) -> AppResult<DateTime<Utc>> {
    let parsed = NaiveDateTime::parse_from_str(date_str, "%d-%m-%Y %H:%M")
        .map_err(|e| AppError::Excel(format!("Invalid date format '{}': {}", date_str, e)))?;
    
    let gregorian_date = parsed.with_year(parsed.year() - BUDDHIST_ERA_OFFSET)
        .ok_or_else(|| AppError::Excel(format!("Year conversion failed for '{}'", date_str)))?;
    
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(gregorian_date, Utc))
}
```

**Examples:**
- `01-03-2567 14:30` (BE) → `2024-03-01T14:30:00Z` (CE)
- `15-12-2568 09:00` (BE) → `2025-12-15T09:00:00Z` (CE)

### 10.4 Parsing Rules

- **Header matching:** Case‑insensitive, trimmed. Required columns: `Qty`, `Code`, `Date Visit`, `Cosume Department`.
- **Sheet selection:** Only sheets whose name matches a `[[products]].id`.
- **Product ID validation:** Column 13 (`Code`) must match sheet name exactly.
- **Date extraction:** Per-row from column 5. Use earliest date in file as `transaction_date` for chronological ordering.
- **Data parsing:**
  - Strip commas from numeric strings.
  - Parse to `Decimal`.
  - Negative values allowed (returns).
  - Zero values **skipped** (not inserted into ledger).
  - Non‑numeric values → log warning, skip row.
- **Missing columns on a sheet:** Toast notification, skip entire sheet.

### 10.5 Configuration Update

The `sheet_name` field in `[[products]]` is **deprecated**. Sheet name now directly equals `product.id`.

```toml
[[products]]
id          = "GAUZE-01"          # Must match Excel sheet name exactly
display_name = "Gauze 500cm"
unit        = "Roll"
subunit     = "cm"
factor      = "500"
track_subunits = true
```

**Validation (fail-fast at startup):**
- Each `product.id` must have a corresponding Excel sheet (warn if missing).
- Sheet names in Excel not in config → skip with toast warning.

---

## 11. Concurrency Model

- **Background worker:** `std::thread::spawn` for Excel parsing, DB queries, and HTML generation.
- **Communication:** `std::sync::mpsc` channel with messages:
  ```rust
  enum WorkerMessage {
      Progress(String),        // e.g., "Parsing sheet GAUZE-01"
      DryRunData(Vec<DryRunRow>),
      Completed(CommitResult),
      Error(String),
  }
  ```
- **UI thread:** `egui` runs on main thread; updates channel receiver in `update()` loop.

---

## 12. Logging & Observability

- **Structured logs** with key‑value pairs, e.g.:
  `[file_hash=abc123] event=ingestion_started filename=March.xlsx`
- **Log rotation:** Daily, keep 7 days, max file size 5 MB.
- **Log file location:** Same directory as `state.db` (executable dir or fallback).

---

## 13. Error Handling Matrix

| Scenario | Behaviour |
|----------|-----------|
| File hash already in `file_history` | Abort, modal dialog "File already processed." |
| Transaction date <= latest in DB | Abort, modal "Process in chronological order." |
| Excel file locked by another process | Abort, "File in use. Please close and retry." |
| Missing required column on all sheets | Abort, "Required column(s) not found." |
| One sheet has missing column | Skip sheet, toast warning, continue with other sheets. |
| Non‑numeric quantity in a row | Skip row, log warning. |
| `factor = 0` or missing in config | Abort on startup, show config error. |
| No product sheets found in Excel | Abort, "No matching sheets." |
| DB write fails (e.g., constraint) | Full rollback, show error, no file move. |
| HTML generation fails after commit | Log error, allow "Regenerate" later. |
| Archive move fails | Log warning, set retry flag. |
| Duplicate `product.id` in config | Abort on startup, "Duplicate product ID: X". |
| Empty `departments` in config | Abort on startup, "At least one department required." |
| Empty Excel sheet (no data rows) | Skip sheet, toast warning, continue. |
| File modification time used as transaction_date | Log warning, toast "Using file date. Verify before confirming." |
| Product ID mismatch (sheet name ≠ column 13) | Skip row, log warning, continue. |
| Invalid Buddhist year in date field | Skip row, log warning, continue. |

---

## 14. Testing Strategy

### 14.1 Unit Tests
- Euclidean modulo with positive/negative dividends.
- Department-scoped modulo behaviour (`product_id + department_id` isolation).
- `factor == 1` path: opening/closing leftovers forced to `0`, output still computed.
- Whole units consumed calculation.
- Decimal parsing and rounding.
- Date parsing: `DD-MM-YYYY HH:MM` format validation.
- Buddhist year conversion: BE → CE (2567 → 2024) and CE → BE (2024 → 2567).
- Product ID validation: sheet name vs column 13 mismatch detection.

### 14.2 Integration Tests
- Process one file → verify ledger entries and `product_totals`.
- Process second file (later date) → verify opening leftover is correct per `product_id + department_id`.
- Migration `v1 -> v2` backfill: verify `product_totals(product_id, department_id)` is rebuilt from ledger.
- Same product across multiple departments and files → composite totals increment correctly.
- Review dry-run shows only `Product + Department` pairs present in the new file.
- Report includes non-zero carry-over rows even if current file has no transaction for that department.
- Attempt out‑of‑order file → must reject.
- Duplicate file hash → reject.
- Product ID mismatch in row → row skipped, warning logged.
- Empty sheet → skipped with toast warning.
- Buddhist year dates → verify correct conversion to Gregorian in DB.

### 14.3 Performance Tests
- Simulate 50,000 ledger rows; ingestion of a new file (500 rows) must complete in <2 seconds on a typical office PC (8th gen i5, SSD).

---

## 15. Build & Distribution

- **Targets:** Windows (MSVC), Linux (musl) – single static binary.
- **Dependencies:** as per v2.5 plus `rust-embed`, `base64`, `sha2`, `calamine`, `rusqlite` (bundled), `egui`/`eframe`, `rfd`, `tera`, `chrono`, `rust_decimal`, `anyhow`, `thiserror`, `tracing` (optional).
- **Thai font:** Embed `THSarabunNew.ttf` (free license) from `assets/`.
- **Config default:** Generated on first run if missing.

---

## 16. Security & Integrity

- **SHA‑256** of entire Excel file for idempotency.
- **Append‑only ledger** – no `UPDATE` or `DELETE` allowed on `inventory_ledger` or `file_history`.
- **No destructive operations** – archive move is a `rename`; original file is never deleted, only moved.

---

## Appendix A: Departments
**Default configuration**
```
[departments]
"[ER] Emergency" = "[205] Emergency"
"[HEMO] Hemodialysis Centre" = "[218] Hemodialysis Centre"
"[XRAY] Radiology Room" = "[XRAY] Radiology Room"
"[CU11] Checkup" = "[CHK] Checkup"
"[211M] OPD MED" = "[211] OPD MED"
"[S111] Surgery" = "[S111] Surgery"
"[Y111] TRUE C INSTITUTE" = "[211E] TRUE C INSTITUTE"
"[G111] Obstetrical & Gynecology" = "[OBST] Obstetrical & Gynecology"
"[C111] Pediatric" = "[211C] Pediatric"
"[4W] WARD 4" = "[4W] WARD 5"
"[C222] Well Baby" = "[211W] Well Baby"
"[K111] Skin Dept." = "[K111] Skin Dept."
"[101] Foreign Countries" = "[101] Foreign Countries"
"[PT] Physical Therapy" = "[PT] Physical Therapy"
"[O111] Orthopedic" = "[S222] Orthopedic"
"[OR] Operation Room" = "[208] Operation Room"
"[NS] Nursery" = "[NS] Nursery"
"[ICU] ICU" = "[ICU] ICU"
"[PP] Post Pratum" = "[PP] Post Pratum"
"[5W] WARD 5" = "[5W] WARD 6"
"[6W] WARD 6" = "[6W] WARD 7"
"[SGI] GI SCOPE" = "[SGI] GI SCOPE"
"[DNT] DENTAL" = "[213] DENTAL"
"[PHA] Pharmacy Room I" = "[105] Pharmacy Room I"
"[CATH] CATH LAB" = "[245] CATH LAB"
```
---

## Appendix B: Products
This is the default products to be use in config file

|id|display_name|unit|subunit|factor|track_subunits|
|--|------------|----|-------|------|--------------|
|2010100256|GLOVE DISPOSABLE SIZE XS/PAIR(คู่)@|PAIR|PAIR|1|false|
|2010100255|GLOVE DISPOSABLE SIZE S/PAIR(คู่)|PAIR|PAIR|1|false|
|2010100254|GLOVE DISPOSABLE SIZE M/PAIR(คู่)@|PAIR|PAIR|1|false|
|2010100253|GLOVE DISPOSABLE SIZE L/PAIR(คู่)@|PAIR|PAIR|1|false|
|2010101323|ET SHEATH ปลอกหุ้ม DIGITAL THERMOMETER TERUMO (ซองปรอท)@|PC|PC|1|false|
|1161106077|Chlorhexidine 2% in alcohol 30ml|BOT|BOT|1|false|
|ABN2100177|MICROPORE 1" ตัดแบ่งเป็น CM(**)|CM.|ROLL|900|true|
|ABN2100178|MICROPORE 1/2" ตัดแบ่งเป็น CM(**)|CM.|ROLL|900|true|
|ABN2100179|MICROPORE 1" (มีที่ตัด) ตัดแบ่งเป็น CM(**)|CM.|ROLL|900|true|
|ABN2100180|MICROPORE 1/2" (มีที่ตัด) ตัดแบ่งเป็น CM(**)|CM.|ROLL|900|true|
|ABN2100176|INNOTAPE (SILICONE TAPE) 2.5CM ตัดแบ่ง 5 CM(**)|5 CM.|ROLL|30|true|
|ABN2100175|Disposible sheet laminate 80X200cm.|PC|PC|1|false|
|ABN2100240|Antiseptic Tower/แผ่น ( No Alcohol )|PC|PC|1|false|
|ABN2100298|Antiseptic Towel /แผ่น|PC|PC|1|false|
|ABN2100302|Chlorhexidine 2% in water 15 ml (คิดเงิน)|15 ml|15 ml|1|false|

---

## Appendix C: Report format guideline (Sample one-product reference)

Reference file: `asset/Sample report one product.pdf`

Use this appendix as the acceptance target for report layout similarity.

### C.1 One-product page structure

For each product, the report should render a dedicated section/page with this order:

1. Product ID (eg `2010100255`)
2. Product display name (eg `GLOVE DISPOSABLE SIZE S/PAIR(คู่)`)
3. Department consumption table
4. Product summary lines for subunit/whole-unit transfer fields

### C.2 Department table format

The main table should follow the sample's shape and labels as closely as practical:

| Consume Department Code |Product name| ยอดยกมา | ขอยืม |เบิก| จ่าย | unit |
|-------------------------|------------|---------|------|---|------|------|

Row behavior:
- `Consume Department Code`: mapped department code + display name (eg `[245] CATH LAB`).
- `Product name`: Product display name (eg `GLOVE DISPOSSABLE SIZE S/PAIR(คู่)`)
- `ยอดยกมา`: opening value for the same `product_id + department_id` context.
- `ขอยืม`: requested/borrowed amount, the tools do not process borrow so its intentional leave empty.
- `เบิก`: dispensed amount used for ledger/report totals, in whole amount
- `จ่าย`: usually it dispense(`เบิก`)-borrowed (`ขอยืม`) since we do not process borrow it is to be leave empty.
- `unit`: product unit from config (eg `PAIR`, `ROLL`, `BOT`).

### C.3 Similarity rules vs sample PDF

- Keep a clean print-first layout on A4 landscape.
- Preserve Thai labels shown in the sample (`ยอดยกมา`, `ขอยืม`, `จ่าย`).
- Ensure one-product readability: product identifier/name must be visually obvious.
- Department rows should be deterministic (stable ordering across regenerations).
- Include report metadata (`generated timestamp`, `file hash`, `report version`) in footer or header without reducing table readability.
- File header should carry over every pages.

### C.4 Practical notes for implementation

- HTML report generated by Tera should mirror this format even if exact pixel matching is not possible.
- Browser print/PDF output is the source of truth for visual validation.
- When format trade-offs are needed, prioritize table clarity and Thai text legibility.
- Try keeping one product per page, avoid overflow, if needed adjust font size to samller
---
**This document (v3.2) is the single source of truth for implementation.**
All conflicts between v2.5 and v3.0 have been resolved. v3.2 integrates the full UI/UX design from Stitch MCP project `netherica design` (Nordic Precision design system). The design is complete, testable, and ready for development.
