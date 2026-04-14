# Netherica v0.2.x - UI Rework Task Checklist

> **Source of truth:**
> - Current runtime UI and behavior in `src/ui/mod.rs`
> - Window/bootstrap wiring in `src/main.rs`
> - Runtime data hooks in `src/storage.rs`, `src/ingestion.rs`, and `src/repository.rs`
> - Local design system reference in `gui_design/nordic_precision/DESIGN.md`
> - Local screen references:
>   - `gui_design/ingestion_1idle/{screen.png,code.html}`
>   - `gui_design/ingestion_3progress/{screen.png,code.html}`
>   - `gui_design/ingestion_2review/{screen.png,code.html}`
>   - `gui_design/ingestion_4complete/{screen.png,code.html}`
>   - `gui_design/settings_departments/{screen.png,code.html}`
>   - `gui_design/settings_products/{screen.png,code.html}`
>
> **Compatibility rules:**
> - Stitch is obsolete for this phase. Do not reference Stitch MCP IDs, screen IDs, or the old Stitch project.
> - `Netherica_rqrmnt.md` is not present in this repo. Do not block implementation on it.
> - When the local mockups conflict with the current codebase direction, preserve the current product direction and use the mockup for layout/style only.
> - Preserve the post-draft behaviors already implemented in `src/ui/mod.rs`: storage fallback warning toast, transaction-date fallback acknowledgement before commit, report auto-open/report-folder actions, report regeneration, archive retry, and the existing UI tests.
> - `gui_design/ingestion_4complete` still shows stale `Inventory` navigation. Current target navigation is `Ingestion`, `Reports`, and `Settings` only.
> - `gui_design/settings_departments` and `gui_design/settings_products` show add/edit affordances, but the current app only exposes read-only configuration data. Treat create/edit controls as future scope unless explicitly added.
> - `gui_design/` has no dedicated Reports screen. Build the Reports landing view from the design system and current runtime capabilities.
> - The crate is already on `0.2.2`. UI rework should not change the release version unless explicitly requested.
>
> **Goal:** Bring the current egui desktop UI up to the local `gui_design/` reference quality while preserving the real runtime behaviors already shipping in the codebase. Tasks are ordered linearly and each step should leave the app compiling.

> **Detailed handoff docs:**
> - Step 1: `gui_rework_task/ui_rework_task_step_1.md`
> - Step 2: `gui_rework_task/ui_rework_task_step_2.md`
> - Step 3: `gui_rework_task/ui_rework_task_step_3.md`
> - Step 4: `gui_rework_task/ui_rework_task_step_4.md`
> - Step 5: `gui_rework_task/ui_rework_task_step_5.md`
> - Step 6: `gui_rework_task/ui_rework_task_step_6.md`
> - Step 7: `gui_rework_task/ui_rework_task_step_7.md`
> - Step 8: `gui_rework_task/ui_rework_task_step_8.md`
> - Step 9: `gui_rework_task/ui_rework_task_step_9.md`

---

### Step 1 - Preserve Current Working Behavior Before Restructure

- [ ] **12.1 - Split `src/ui/mod.rs` into focused sub-modules** - The UI is still a monolithic ~989-line file. Extract it into a structure that matches the current app, not the older draft assumptions:
    - `ui/mod.rs` - `NethericaApp`, top-level routing, shared state, `eframe::App` impl
    - `ui/theme.rs` - font registration and egui design-system setup
    - `ui/sidebar.rs` - left navigation and shell chrome
    - `ui/components.rs` - reusable buttons, cards, modal/toast helpers
    - `ui/worker.rs` - `WorkerMessage`, `start_ingestion_worker()`, `start_commit_worker()`, worker-side helpers
    - `ui/views/mod.rs` - view re-exports
    - `ui/views/idle.rs` - idle/import view
    - `ui/views/progress.rs` - parsing + committing in-flight views
    - `ui/views/dry_run.rs` - review table and metrics
    - `ui/views/complete.rs` - completion summary/actions
    - `ui/views/settings.rs` - departments/products settings pages
    - `ui/views/reports.rs` - reports landing/placeholder
    - Preserve existing functions and behaviors such as `maybe_show_storage_fallback_warning()`, `handle_report_ready()`, `open_report_folder_action()`, `open_latest_report_action()`, `regenerate_last_report()`, and the archive retry action currently exposed from the Complete state.
    - **Verification:** `cargo check` and `cargo test` must pass with zero regressions.

- [ ] **12.2 - Move and update the existing UI tests during the split** - The current UI module already contains 8 tests, not 7. Migrate them into the new module layout and keep coverage for:
    - transaction-date fallback acknowledgement gating
    - report guidance message generation
    - report folder resolution
    - storage fallback warning behavior
    - font registration / fallback preservation
    - **Verification:** `cargo test` passes after the split.

### Step 2 - App Shell, Navigation, and Bootstrap

- [ ] **12.3 - Add real navigation state** - Define `NavigationSection` and `SettingsTab` enums and wire them into `NethericaApp` so the shell routes between:
    - `Ingestion`
    - `Reports`
    - `Settings`
    - Remove the current stale `Inventory` sidebar item entirely.

- [ ] **12.4 - Update the window bootstrap without changing the crate version** - In `src/main.rs`:
    - Add `pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");`
    - Update the native window title to `Netherica | Pharmacy Reconciliation`
    - Increase the default window size to fit the left rail + content workspace comfortably
    - Add a minimum size appropriate for the redesigned shell
    - Do **not** change `Cargo.toml` version as part of this UI pass
    - **Verification:** `cargo check`

- [ ] **12.5 - Redesign the global shell to match the current local references** - Replace the placeholder shell with a cohesive app frame inspired by `gui_design/ingestion_1idle` and the settings screens:
    - left sidebar with 3-item navigation
    - top/header treatment consistent with the dark editorial style
    - bottom status/footer strip for current app state + status message
    - no duplicated in-content title like `Netherica v0.2.2 - Ingestion System`
    - **Verification:** `cargo check`

### Step 3 - Runtime Summary State for Idle and Completion Views

- [ ] **12.6 - Load startup summary data for the Idle view** - During app initialization:
    - open the database when possible and query `Repository::get_max_transaction_date()`
    - store `last_run_timestamp: Option<DateTime<Utc>>`
    - store `db_connected: bool`
    - store `storage_source: Option<DataRootSource>` from `DataDirectory::resolve()`
    - preserve the existing storage fallback warning flow instead of replacing it
    - **Verification:** `cargo check`

- [ ] **12.7 - Add structured parsing/progress view state** - Extend `NethericaApp` with fields needed for `gui_design/ingestion_3progress`:
    - `parsing_logs: Vec<(String, String, String)>` for timestamp/level/message
    - `parsing_file_metadata: Option<ParsingFileMetadata>` containing filename, file size, sheet count, and sheet names
    - `parsing_progress: Option<(String, usize, usize)>` for current sheet and row counts
    - clear or reset these fields at the correct state boundaries
    - **Verification:** `cargo check`

- [ ] **12.8 - Add completion summary fields without regressing current report flows** - Extend `NethericaApp` with:
    - `completed_rows_processed: usize`
    - `completed_filename: String`
    - `completed_file_hash: String`
    - `completed_archive_move_pending: bool`
    - `pipeline_start: Option<std::time::Instant>`
    - `dry_run_elapsed: Option<std::time::Duration>`
    - preserve current `last_report_path` and `post_generation_guidance`
    - capture completion data before consuming or clearing pending state
    - **Verification:** `cargo check`

- [ ] **12.9 - Add small helper metrics for the review and completion screens** - Introduce helpers for:
    - warning count in dry-run rows
    - clean/non-warning count or accuracy summary
    - formatting elapsed durations and truncated hashes for display
    - **Verification:** `cargo check`

### Step 4 - Worker and Message Refactor

- [ ] **12.10 - Extend `WorkerMessage` with structured progress variants** - Add structured variants for parsing metadata/log/progress updates, while keeping the current general-purpose progress path available until the full refactor is complete:
    ```rust
    ParsingStarted { filename: String, file_size: u64, sheet_count: usize, sheet_names: Vec<String> },
    ParsingLog { timestamp: String, level: String, message: String },
    ParsingProgress { current_sheet: String, rows_processed: usize, total_rows: usize },
    DryRunTimingComplete { elapsed: std::time::Duration },
    ```
    - Keep `Progress(String)` for legacy/general status updates, especially for commit-side messaging.
    - **Verification:** `cargo check`

- [ ] **12.11 - Refactor the ingestion worker to emit structured parsing events** - Update the worker and parser path so the UI can render the local progress reference instead of a generic spinner:
    - record `pipeline_start` when ingestion begins
    - emit file metadata once the workbook is opened
    - emit per-sheet logs and row progress during parsing
    - emit dry-run timing once preparation completes
    - **Verification:** `cargo check` and `cargo test`

- [ ] **12.12 - Update `process_worker_messages()` without regressing completion side effects** - The current completion pipeline already does useful work; keep it intact while enriching the UI state:
    - `Completed` must still call `handle_report_ready()`
    - archive-pending outcomes must still produce the existing warning/toast semantics
    - parsing/progress payloads must update the new structured fields
    - **Verification:** `cargo check`

### Step 5 - Local Design System Primitives

- [ ] **12.13 - Extract the full design-token set from the local design references** - Move the current inline theme values into named constants driven by `gui_design/nordic_precision/DESIGN.md` and the shared palette embedded in the local `code.html` files:
    - surface hierarchy tokens
    - primary/secondary/tertiary/error palette entries
    - on-colors and outline tokens
    - inverse tokens where useful
    - **Verification:** `cargo check`

- [ ] **12.14 - Update the font stack to Inter + Thai fallback** - Align the desktop UI with the current design system rather than the older draft:
    - `asset/fonts/Sarabun/Sarabun-Regular.ttf` already exists and can remain the Thai-capable fallback
    - bundle local Inter font files if needed for the primary headline/body stack
    - preserve UTF-8 and Thai rendering coverage from the existing font test path
    - **Verification:** `cargo check` and `cargo test`

- [ ] **12.15 - Implement shared component primitives in `components.rs`** - Add reusable helpers consistent with the local refs:
    - primary, secondary, and ghost buttons
    - status cards and metric cards
    - pill tabs / chips for settings and state labels
    - consistent section headers and info callouts
    - **Verification:** `cargo check`

- [ ] **12.16 - Restyle toast, error, and report-ready overlays** - The behaviors already exist; update the presentation to the local design language:
    - toast anchored top-right with longer auto-dismiss and explicit close affordance
    - error modal with dark overlay / glass treatment
    - report-ready modal with polished actions for `Open Report`, `Open Report Folder`, and `Close`
    - **Verification:** `cargo check`

### Step 6 - Ingestion Workflow Views

- [ ] **12.17 - Redesign the Idle view using `gui_design/ingestion_1idle`** - Replace the current grouped form with a polished landing view that still supports the real workflow:
    - prominent upload CTA
    - summary/status cards backed by live app data
    - no raw config dump or generic `ui.group()` blocks
    - the selected file and start-ingestion action should still be clear and usable
    - **Verification:** `cargo check`

- [ ] **12.18 - Redesign in-flight processing using `gui_design/ingestion_3progress`** - The current app has both `Parsing` and `Committing` states, but only one local progress reference. Use the same visual shell for both:
    - `Parsing` shows file metadata, parsing log stream, and row progress
    - `Committing` reuses the same shell with commit-specific copy/status and no unsafe user actions
    - remove the current generic animated `ProgressBar::new(0.5)` placeholder treatment
    - **Verification:** `cargo check`

- [ ] **12.19 - Redesign the Dry Run review view using `gui_design/ingestion_2review`** - Keep the current domain behavior while restyling the screen:
    - summary metric cards across the top
    - restyled `TableBuilder` without heavy grid-line noise
    - fallback-acknowledgement warning remains visible and required when `transaction_date_fallback_used`
    - footer actions for cancel / confirm-and-generate-report remain intact
    - **Verification:** `cargo check`

- [ ] **12.20 - Redesign the Completion view using `gui_design/ingestion_4complete`** - Match the success-card layout while keeping current functionality that the mock does not fully cover:
    - success hero + completion metrics
    - keep `Open Report Folder`, `Regenerate Last Report`, and `New File`
    - keep an explicit archive retry affordance even though the local mock lacks it
    - show metadata such as execution time, archive status, validator/app version, and truncated file hash
    - **Verification:** `cargo check`

### Step 7 - Reports and Settings Routing

- [ ] **12.21 - Implement Settings tab routing inside the new shell** - When `active_section == Settings`, render a tab/pill switcher between `Departments` and `Products` and route the content accordingly.

- [ ] **12.22 - Implement the Departments view using `gui_design/settings_departments` as a read-only adaptation** - Use the card-grid visual language, but bind it to the real config shape that exists today:
    - one card per `config.departments` entry
    - emphasize department code and mapped display name
    - do not add create/edit flows in this pass
    - **Verification:** `cargo check`

- [ ] **12.23 - Implement the Products view using `gui_design/settings_products` as a read-only adaptation** - Bind the layout to the actual fields in `Config::products`:
    - show `id`, `display_name`, `unit`, `subunit`, `factor`, and `track_subunits`
    - use card-based presentation matching the local design language
    - do not add create/edit flows in this pass
    - **Verification:** `cargo check`

- [ ] **12.24 - Implement the Reports landing view without a dedicated mock** - Build a clean design-system-consistent reports page that fits the current product scope:
    - headline and supporting copy
    - explain the currently available report actions
    - optionally surface latest generated report context if already available in app state
    - **Verification:** `cargo check`

### Step 8 - Cleanup and Consistency Pass

- [ ] **12.25 - Remove leftover generic separators and group boxes where they fight the new design system** - Audit the UI and replace line-heavy layout patterns with spacing, tonal layering, and `egui::Frame` usage.

- [ ] **12.26 - Apply a typography, spacing, and hover-state audit across every view** - Align all screens to the current local design system:
    - consistent headline/body/label hierarchy
    - correct surface nesting
    - restrained hover transitions
    - Thai text remains legible and correctly rendered
    - **Verification:** `cargo check`

- [ ] **12.27 - Remove stale UI copy and shell leftovers** - Final sweep to ensure the codebase no longer exposes outdated draft artifacts such as:
    - `Inventory` navigation
    - placeholder shell labels
    - duplicated versioned titles inside the main content area
    - **Verification:** `cargo check`

### Step 9 - Validation and Visual QA

- [ ] **12.28 - Compilation and lint gate** - Run the full validation suite:
    - `cargo check`
    - `cargo test`
    - `cargo clippy --all-targets --all-features -- -D warnings`
    - `cargo build --release`

- [ ] **12.29 - Runtime smoke test: full ingestion flow** - Launch the app and verify:
    1. Window title/size and shell render correctly.
    2. Idle view shows the redesigned CTA and live status cards.
    3. File selection transitions into the new progress/parsing view with metadata and logs.
    4. Dry run review shows summary cards, styled table, and fallback acknowledgement when applicable.
    5. Confirming commit transitions through the in-flight commit shell into the redesigned completion screen.
    6. Completion actions still support report folder, report regeneration, archive retry, and new-file reset.

- [ ] **12.30 - Runtime smoke test: navigation, modal, and warning flows** - Verify:
    1. Sidebar switches cleanly among Ingestion, Reports, and Settings.
    2. Settings switches cleanly between Departments and Products.
    3. Duplicate-file or other failure paths show the redesigned error modal.
    4. Storage fallback warning toast still appears only once.
    5. Report-ready modal still supports open report / open folder / close.

- [ ] **12.31 - Visual QA against the local design references** - Compare each implemented view against `gui_design/*/screen.png` and `code.html`:
    - surface hierarchy matches the editorial dark theme
    - buttons, cards, chips, and tables follow the local design language
    - no leftover line-heavy enterprise styling where the local reference expects tonal layering
    - Thai text renders correctly
    - any local mock detail that conflicts with current product direction has been consciously adapted rather than copied blindly
