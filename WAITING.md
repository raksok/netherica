# Netherica v0.2 – Implementation Task Checklist

> **Source of truth:**
> - Stitch MCP project `netherica design` (ID `18330809547273064391`) — 7 canonical screens
> - `DESIGN.md` — Full design tokens, component specs, do's/don'ts
> - `Netherica_rqrmnt.md` section 7 — View-by-view UI specification
>
> **Goal:** Transform the current functional-but-minimal `src/ui/mod.rs` into a polished, design-system-compliant UI matching the Stitch prototypes.
> Tasks are ordered **linearly** — each task builds on the previous and must be completed sequentially.

---

### Step 1 — Module Decomposition & Compilation Gate

- [ ] **12.1 – Split `ui/mod.rs` into sub-modules** — Extract the monolithic 985-line `src/ui/mod.rs` into focused files. **All existing behavior must be preserved identically** — this is a pure structural refactor with zero logic changes:
    - `ui/mod.rs` — Re-exports, `NethericaApp` struct, `eframe::App` impl with top-level layout shell
    - `ui/theme.rs` — `apply_design_system()`, `configure_egui_fonts()`, font constants
    - `ui/sidebar.rs` — Sidebar panel rendering
    - `ui/views/mod.rs` — View sub-module re-exports
    - `ui/views/idle.rs` — `render_idle_view()`
    - `ui/views/parsing.rs` — Parsing spinner view
    - `ui/views/dry_run.rs` — `render_dry_run_view()`
    - `ui/views/complete.rs` — Completion view
    - `ui/components.rs` — Reusable components (initially empty, populated later)
    - `ui/worker.rs` — `WorkerMessage` enum, `start_ingestion_worker()`, `start_commit_worker()`
    - **Verification:** Run `cargo check` and `cargo test` — must pass with zero errors.

- [ ] **12.2 – Migrate existing UI tests** — Move the 7 unit tests from the old monolithic `mod.rs` into the appropriate new sub-modules (e.g., `worker.rs` for commit tests, `theme.rs` for font test, `mod.rs` for utility tests). Run `cargo test` — all 7 tests must pass.

### Step 2 — App State Enrichment

- [ ] **12.3 – Add `NavigationSection` and `SettingsTab` enums** — Define `enum NavigationSection { Ingestion, Reports, Settings }` and `enum SettingsTab { Departments, Products }` in `ui/mod.rs`. Add `active_section: NavigationSection` and `active_settings_tab: SettingsTab` fields to `NethericaApp`. Default to `Ingestion`/`Departments`. Wire `active_section` into the central panel match so switching sections renders different content areas.

- [ ] **12.4 – Define `APP_VERSION` constant** — Add `pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");` to `main.rs`. Update `Cargo.toml` version to `"0.2.0"`. Used by the Completion view and window title.

- [ ] **12.5 – Update `eframe::NativeOptions` window config** — In `main.rs`, update:
    - Window title: `"Netherica — Pharmacy Reconciliation"` (drop version from title bar).
    - Default size: `[1280.0, 800.0]` to accommodate sidebar + content.
    - Minimum size: `[960.0, 600.0]`.
    - **Verification:** `cargo check`.

- [ ] **12.6 – Query startup data for Idle view** — On `NethericaApp::new()`:
    - Open DB (`Database::new`), query `repository.get_max_transaction_date()` → store as `last_run_timestamp: Option<DateTime<Utc>>`.
    - Store `db_connected: bool` (true on success, false on failure — don't crash).
    - Store `storage_source: Option<DataRootSource>` from `DataDirectory::resolve()`.
    - These power the Idle view's 3 status cards. **Verification:** `cargo check`.

- [ ] **12.7 – Add structured parsing state fields** — Add to `NethericaApp`:
    - `parsing_logs: Vec<(String, String, String)>` — (timestamp, level, message)
    - `parsing_file_metadata: Option<ParsingFileMetadata>` — struct with `filename: String`, `file_size: u64`, `sheet_count: usize`, `sheet_names: Vec<String>`
    - `parsing_progress: Option<(String, usize, usize)>` — (current_sheet, rows_done, total_rows)
    - Clear all on state transition to Idle. **Verification:** `cargo check`.

- [ ] **12.8 – Add completion outcome fields** — Add to `NethericaApp`:
    - `completed_rows_processed: usize`
    - `completed_filename: String`
    - `completed_file_hash: String`
    - `pipeline_start: Option<std::time::Instant>` — set when ingestion begins
    - `dry_run_elapsed: Option<std::time::Duration>` — set when dry run completes
    - Capture `pending_commit.ledger_entries.len()`, `pending_commit.filename`, and truncated `pending_commit.file_hash` BEFORE consuming the commit. **Verification:** `cargo check`.

### Step 3 — Worker Message Enrichment & Ingestion Refactor

- [ ] **12.9 – Extend `WorkerMessage` enum with structured variants** — Add to `WorkerMessage`:
    ```rust
    ParsingStarted { filename: String, file_size: u64, sheet_count: usize, sheet_names: Vec<String> },
    ParsingLog { timestamp: String, level: String, message: String },
    ParsingProgress { current_sheet: String, rows_processed: usize, total_rows: usize },
    DryRunTimingComplete { elapsed: std::time::Duration },
    ```
    Keep existing `Progress(String)` for backward compat. **Verification:** `cargo check`.

- [ ] **12.10 – Refactor `start_ingestion_worker` for structured progress** — Modify the worker thread to:
    1. Record `Instant::now()` before `prepare_ingestion_dry_run`.
    2. After opening workbook (requires modifying `parse_excel_file` signature to accept `mpsc::Sender<WorkerMessage>` or a callback), emit `ParsingStarted` with file metadata.
    3. Emit `ParsingLog` for significant events (sheet opened, column mapped, warnings).
    4. Emit `ParsingProgress` per-sheet with row counts.
    5. After dry run completes, emit `DryRunTimingComplete { elapsed }`.
    - Also set `pipeline_start` on the app when `start_ingestion_worker` is called.
    - **Verification:** `cargo check` and `cargo test`.

- [ ] **12.11 – Update `process_worker_messages` handler** — Extend the message loop to handle:
    - `ParsingStarted` → populate `parsing_file_metadata`
    - `ParsingLog` → append to `parsing_logs`
    - `ParsingProgress` → update `parsing_progress`
    - `DryRunTimingComplete` → store `dry_run_elapsed`
    - `Completed` → capture `completed_rows_processed`, `completed_filename`, `completed_file_hash` from current state before clearing.
    - **Verification:** `cargo check`.

### Step 4 — Design System Primitives

- [ ] **12.12 – Define full color token constants in `theme.rs`** — Create typed `const` values for every token from `DESIGN.md` Section 2 (~40 tokens). Current code defines ~10 tokens inline in `apply_design_system()`. Move all to named constants:
    - All 8 surface tokens (surface-container-lowest through surface-bright)
    - All primary, secondary, tertiary, error palette entries
    - All on-colors, outline-variant
    - Inverse tokens
    - **Verification:** `cargo check`. Existing `apply_design_system()` refactored to use the new constants.

- [ ] **12.13 – Update font configuration for Inter + Sarabun** — Per `DESIGN.md` section 3:
    - Download and bundle `Inter-Regular.ttf` and `Inter-SemiBold.ttf` into `asset/` (free, OFL license).
    - Update `configure_egui_fonts()` in `theme.rs`: load Inter as primary proportional font, NotoSansThaiLooped (current) or Sarabun-Regular (already in `asset/`) as Thai fallback.
    - Keep monospace as default egui monospace.
    - **Verification:** `cargo check`. The font test must still pass.

- [ ] **12.14 – Implement button variant helpers in `components.rs`** — Per `DESIGN.md` 6.1:
    - `fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response` — `primary-container` (`#88c0d0`) fill, `on-primary` text, `radius-md` (6px).
    - `fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response` — `surface-container-highest` fill, `on-surface` text, `radius-md`.
    - `fn ghost_button(ui: &mut egui::Ui, text: &str) -> egui::Response` — transparent fill, `primary` text, `radius-md`.
    - **Verification:** `cargo check`.

- [ ] **12.15 – Implement `StatusCard` and `MetricCard` in `components.rs`** — Per `DESIGN.md` 6.4:
    - `fn status_card(ui: &mut egui::Ui, label: &str, value: &str)` — `surface-container-low` background, `radius-xl` (12px), label in `label-md` uppercase, value in `body-md`. Hover → `surface-container-highest` + ghost border.
    - `fn metric_card(ui: &mut egui::Ui, label: &str, value: &str)` — Same base, value in `headline-lg` (32px bold).
    - **Verification:** `cargo check`.

- [ ] **12.16 – Upgrade toast notification system** — Per `Netherica_rqrmnt.md` 7.5:
    - Move anchor from `RIGHT_BOTTOM` to `RIGHT_TOP`.
    - Increase auto-dismiss from 3s to 5s.
    - Style: `surface-container-highest` fill, `radius-xl`, ambient shadow.
    - Add dismiss X button using `ghost_button`.
    - Extract into `fn render_toast(ctx, toast_message)` in `components.rs`.
    - **Verification:** `cargo check`.

- [ ] **12.17 – Upgrade error modal system** — Per `Netherica_rqrmnt.md` 7.5:
    - Render a semi-transparent dark overlay using `egui::Area` with `surface-variant` at 70% alpha.
    - Modal card: `surface-container-high` fill, `radius-xl`, ambient shadow.
    - Use `error` color (`#ffb4ab`) for error title/icon, `on-surface` for body.
    - Buttons: `primary_button` for acknowledge, `secondary_button` for retry.
    - Extract into `fn render_error_modal(ctx, critical_error)` in `components.rs`.
    - **Verification:** `cargo check`.

### Step 5 — Sidebar Redesign

- [ ] **12.18 – Redesign sidebar panel** — In `sidebar.rs`, implement per `Netherica_rqrmnt.md` 7.2:
    - Background: `surface-container-low` (`#161c27`).
    - Logo: "Netherica" in `headline-md` (24px semibold, `on-surface`), "Pharmacy Reconciliation" in `body-sm` (13px, `on-surface-variant`).
    - Navigation items (3): Ingestion, Reports, Settings — clicking updates `active_section`.
    - Active item: `primary` text + `surface-container` background.
    - Inactive items: `on-surface-variant` text + transparent background.
    - Remove the "Inventory" nav item.
    - Footer: "Precision Reconciliation Engine" in `label-sm`, bottom-pinned.
    - **Verification:** `cargo check`.

### Step 6 — Footer & Global Polish

- [ ] **12.19 – Remove all `ui.separator()` calls** — Audit entire UI codebase and replace every `ui.separator()` with `ui.add_space(16.0)`. Enforces the No-Line Rule from `DESIGN.md`.

- [ ] **12.20 – Replace all `ui.group()` with `egui::Frame`** — Replace `ui.group()` (draws bordered rectangle) with `egui::Frame` using `surface-container-low` fill, `radius-xl` rounding, no stroke.

- [ ] **12.21 – Redesign footer/status bar** — Replace bottom separator + status text with:
    - `egui::TopBottomPanel::bottom` with `surface-container-low` fill.
    - Left: "Precision Reconciliation Engine" in `label-sm`.
    - Right: current state + status message in `label-sm`.
    - **Verification:** `cargo check`.

### Step 7 — View-by-View Redesign (Ingestion Workflow)

- [ ] **12.22 – Redesign Idle view** — In `views/idle.rs`, implement per `Netherica_rqrmnt.md` 7.3.1 / Stitch screen `fcc6181c`:
    - Headline: "Ready for new reconciliation run?" (`headline-lg`, `on-surface`).
    - Description: "Upload your latest Pharmacy Excel export..." (`body-md`, `on-surface-variant`).
    - File upload button: `primary_button("Select Excel File")` — triggers `rfd` picker.
    - Status Cards row (3x `status_card`):
        - **Last Run:** `last_run_timestamp` formatted, or "No files processed".
        - **Sync State:** `db_connected` + `storage_source` status.
        - **Config Status:** product/department count from `config`.
    - Remove raw config dump and `ui.group` blocks.
    - **Verification:** `cargo check`.

- [ ] **12.23 – Redesign Parsing view** — In `views/parsing.rs`, implement per `Netherica_rqrmnt.md` 7.3.2 / Stitch screen `ba375e7c`:
    - Headline: "Analyzing Data Structure" (`headline-lg`).
    - Sub-headline: "Parsing sheet: {name}..." from `parsing_progress`.
    - File Metadata Card (`surface-container` fill): from `parsing_file_metadata`.
    - Live log area: `egui::ScrollArea` with `surface-container-lowest` background, monospace font, entries from `parsing_logs`. Warnings in `tertiary` color.
    - Progress bar: `egui::ProgressBar` with `primary` fill on `surface-container-highest` track.
    - No user actions during parsing.
    - **Verification:** `cargo check`.

- [ ] **12.24 – Add domain metrics for Review view** — Add helper functions:
    - `fn compute_warning_count(rows: &[DryRunRow]) -> usize` — count products where `closing_leftover < 0`.
    - `fn compute_accuracy_score(rows: &[DryRunRow]) -> f64` — percentage of products with non-negative closing leftover.
    - Place in `views/dry_run.rs` or `domain.rs`. **Verification:** `cargo check`.

- [ ] **12.25 – Redesign Dry Run (Review) view** — In `views/dry_run.rs`, implement per `Netherica_rqrmnt.md` 7.3.3 / Stitch screen `1f6aa1e1`:
    - Headline: "Dry Run Review" (`headline-lg`).
    - Summary Metric Cards row (3x `metric_card`): product count, warning count (from 12.24), computation time (from `dry_run_elapsed`).
    - Table: existing `TableBuilder` restyled:
        - Header: `surface-container-low` background, `label-sm` uppercase `on-surface-variant`.
        - Rows: 24px height, no grid lines, hover → `surface-container-high`.
    - Row count label: "Showing {n} of {total} reconciled products".
    - Confirmation footer:
        - Warning text in `body-sm`, `on-surface-variant`.
        - `ghost_button("Cancel")` + `primary_button("Confirm & Generate Report")`.
    - **Verification:** `cargo check`.

- [ ] **12.26 – Redesign Completion view** — In `views/complete.rs`, implement per `Netherica_rqrmnt.md` 7.3.4 / Stitch screen `78a279e9`:
    - Headline: "Process Completion" (`headline-lg`).
    - Success banner: "Reconciliation Successful" (`headline-md`, `primary`).
    - Summary metrics inline (2x `metric_card`): Rows Processed (`completed_rows_processed`), Discrepancies (0 for clean runs).
    - Action section (2x `secondary_button`): "Open Report Folder", "Regenerate Last Report".
    - New Cycle CTA: `primary_button("New File")` → resets to Idle.
    - System Health footer strip (`surface-container-low`):
        - Execution Time: `pipeline_start.elapsed()`
        - Data Integrity: 100% (ACID success)
        - Validator Version: `APP_VERSION`
        - Log Hash: truncated `completed_file_hash`
    - **Verification:** `cargo check`.

### Step 8 — Settings Views (New Feature)

- [ ] **12.27 – Implement Settings navigation routing** — When `active_section == Settings`, render a tab bar with "Departments" and "Products" pills. Active tab updates `active_settings_tab`. Render the corresponding sub-view below.

- [ ] **12.28 – Implement Departments view** — In `views/settings.rs`, per `Netherica_rqrmnt.md` 7.4.1 / Stitch screen `746808a3`:
    - Headline: "Department Configuration" (`headline-lg`).
    - Card grid: one card per `config.departments` entry. `surface-container-low` fill, `radius-xl`. Code in `label-md` uppercase monospace, display name in `headline-sm`.
    - Read-only (v1) — display current config state only.
    - **Verification:** `cargo check`.

- [ ] **12.29 – Implement Products view** — In `views/settings.rs`, per `Netherica_rqrmnt.md` 7.4.2 / Stitch screen `2a3ea4a0`:
    - Headline: "Product Configuration" (`headline-lg`).
    - Card grid: one card per `config.products`. `surface-container-low` fill, `radius-xl`. ID in `label-md` monospace, name in `headline-sm`, details (unit, subunit, factor, track_subunits) in `body-sm`.
    - System Status footer: product count.
    - Read-only (v1).
    - **Verification:** `cargo check`.

- [ ] **12.30 – Implement Reports placeholder view** — When `active_section == Reports`, show:
    - Headline: "Reports" (`headline-lg`).
    - Message: "Report browsing will be available in a future release. Use 'Open Report Folder' from the completion screen."
    - This ensures all 3 nav sections route properly.
    - **Verification:** `cargo check`.

### Step 9 — Typography & Final Polish

- [ ] **12.31 – Apply typography hierarchy audit** — Sweep all views and apply consistent text styling:
    - `headline-lg` (32px): page titles → `RichText::new(...).size(32.0).strong()`.
    - `headline-md` (24px): section headings → `RichText::new(...).size(24.0)`.
    - `headline-sm` (20px): sub-headings → `RichText::new(...).size(20.0)`.
    - `body-md` (14px): primary text → default.
    - `body-sm` (13px): secondary text → `RichText::new(...).size(13.0)`.
    - `label-md` (12px): card labels, uppercase → `RichText::new(...).size(12.0).strong()`.
    - `label-sm` (11px): table headers, micro-labels.
    - Apply `on-surface` for primary text, `on-surface-variant` for secondary.
    - **Verification:** `cargo check`.

- [ ] **12.32 – Post-generation guidance modal redesign** — Restyle the "Report Ready" modal to match the error modal treatment:
    - Glassmorphism overlay, `surface-container-high` card, `radius-xl`.
    - Use `primary_button("Open Report")`, `secondary_button("Open Folder")`, `ghost_button("Close")`.
    - **Verification:** `cargo check`.

### Step 10 — Integration Testing & Visual QA

- [ ] **12.33 – Compilation and test gate** — Run full suite:
    - `cargo check` — zero errors.
    - `cargo test` — all existing + new tests pass.
    - `cargo clippy` — no warnings in `ui/` modules.
    - `cargo build --release` — successful release build.

- [ ] **12.34 – Runtime smoke test: full ingestion flow** — Launch the application and manually verify the complete ingestion lifecycle:
    1. App launches with correct window title and size.
    2. Sidebar shows 3 items (Ingestion active by default), footer visible.
    3. Idle view: headline, description, file picker button, 3 status cards render.
    4. Select an `.xlsx` file → transition to Parsing view with metadata card and log.
    5. Parsing completes → Review view with metric cards and styled table.
    6. Confirm → Committing → Completion view with metrics and action buttons.
    7. "New File" returns to Idle. Last-run card updates.

- [ ] **12.35 – Runtime smoke test: Settings and navigation** — Verify:
    1. Click "Settings" in sidebar → Departments tab renders with cards.
    2. Click "Products" tab → Products cards render.
    3. Click "Reports" → placeholder message renders.
    4. Click "Ingestion" → returns to workflow.

- [ ] **12.36 – Runtime smoke test: error and toast flows** — Verify:
    1. Process a duplicate file → error modal appears with overlay, acknowledge closes it.
    2. Process a file with date fallback → warning toast appears top-right, auto-dismisses after 5s.
    3. Toast dismiss (X) button works.

- [ ] **12.37 – Visual QA pass** — Compare each view against Stitch screenshots and `DESIGN.md`:
    - No visible borders (No-Line Rule).
    - All backgrounds use correct surface token hierarchy.
    - Hover states shift to `surface-container-high`.
    - Button variants use correct styles (primary/secondary/ghost).
    - Toast positioned top-right, modal has overlay.
    - Typography hierarchy is visually distinguishable.
    - Thai text renders correctly (test with Thai product names).
    - Fix any mismatches found.