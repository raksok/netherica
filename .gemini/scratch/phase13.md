
## Phase 13: State & Logic Binding — Data Pipeline for UI Views

> **Purpose:** Phase 12 redesigns the visual UI, but several views depend on data and state that the current backend does not expose or track.
> This phase bridges the gap between the domain/ingestion layer and the new UI views.

### 13.A – App State Enrichment

- [ ] **13.1 – Add `NavigationSection` and `SettingsTab` enums to app state** — Define `NavigationSection { Ingestion, Reports, Settings }` and `SettingsTab { Departments, Products }`. Add `active_section: NavigationSection` and `active_settings_tab: SettingsTab` fields to `NethericaApp`. Default: `Ingestion` / `Departments`.

- [ ] **13.2 – Query last-run timestamp on startup** — On app initialization (`NethericaApp::new`), open the database and query `repository.get_max_transaction_date()`. Store result as `last_run_timestamp: Option<DateTime<Utc>>` in `NethericaApp`. Display in Idle view's "Last Run" status card.

- [ ] **13.3 – Track database connection status** — Add `db_connected: bool` to `NethericaApp`. Set to `true` on successful `Database::new()` call during startup, `false` on failure. Also store `storage_source: DataRootSource` so the Idle view's "Sync State" card can show "Connected" or "Fallback path used".

- [ ] **13.4 – Define `APP_VERSION` constant** — Add `pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");` (or a hardcoded `"v0.2.0-nord"`) to `main.rs` or a dedicated `version.rs`. Used by the Completion view's "Validator Version" metadata field.

### 13.B – Worker Message Enrichment

- [ ] **13.5 – Add structured parsing progress messages** — Replace the current `WorkerMessage::Progress(String)` with richer variants:
    ```
    WorkerMessage::ParsingStarted { filename: String, file_size: u64, sheet_count: usize, sheet_names: Vec<String> }
    WorkerMessage::ParsingLog { timestamp: String, level: LogLevel, message: String }
    WorkerMessage::ParsingProgress { current_sheet: String, rows_processed: usize, total_rows: usize }
    WorkerMessage::Progress(String)  // keep for general status
    ```
    Update the `parse_excel_file` call site in `start_ingestion_worker` to send these structured messages as parsing progresses.

- [ ] **13.6 – Add parsing log state to `NethericaApp`** — Add fields to hold structured parsing data:
    - `parsing_logs: Vec<(String, LogLevel, String)>` — timestamp, level, message triples
    - `parsing_file_metadata: Option<ParsingFileMetadata>` — struct with filename, file_size, sheet_count, sheet_names
    - `parsing_progress: Option<(String, usize, usize)>` — (current_sheet, rows_done, total_rows)
    - Clear these fields when entering Idle state.

- [ ] **13.7 – Track dry-run computation time** — In `start_ingestion_worker`, record `std::time::Instant::now()` before calling `prepare_ingestion_dry_run`. Send elapsed duration via a new `WorkerMessage::DryRunReady { elapsed: Duration }` or embed it in `DryRunPrepared`. Store as `dry_run_elapsed: Option<Duration>` in `NethericaApp`.

### 13.C – Domain Metrics for Review & Completion Views

- [ ] **13.8 – Compute inventory warning count** — After `build_dry_run_rows`, derive a warning count: products where `closing_leftover` is negative or where `total_subunits_used` exceeds a threshold (e.g., opening + new usage is < 0, indicating stock-out). Store as a field in `PendingIngestionCommit` or compute in UI from `dry_run_data`.

- [ ] **13.9 – Compute accuracy/variance metric** — Add a simple accuracy metric: for example, percentage of products with closing leftover within expected range. This can be computed entirely from `dry_run_rows` in the UI layer without new backend code. Add a helper function `fn compute_accuracy_score(rows: &[DryRunRow]) -> f64` in `ui/views/dry_run.rs`.

- [ ] **13.10 – Track total pipeline execution time** — Record `Instant::now()` when the user clicks "Select Excel File" (or when ingestion starts). Store as `pipeline_start: Option<Instant>` in `NethericaApp`. On `AppState::Complete`, compute elapsed as `pipeline_start.elapsed()`. Display in Completion view's "Execution Time" metadata.

- [ ] **13.11 – Preserve outcome metrics for Completion view** — After `WorkerMessage::Completed(outcome)`, store:
    - `completed_rows_processed: usize` (from `pending_commit.ledger_entries.len()`)
    - `completed_filename: String` (from `pending_commit.filename`)
    - `completed_file_hash: String` (truncated, for "Log Hash" display)
    These must be captured before `pending_commit` is consumed by the commit worker.

### 13.D – Data Integrity & Reports Section Placeholder

- [ ] **13.12 – Compute data integrity percentage** — Define as: `(rows_successfully_committed / total_rows_parsed) * 100`. Since the current ACID commit is all-or-nothing, this is effectively `100%` on success or an error state. Compute and store alongside outcome metrics.

- [ ] **13.13 – Reports section placeholder** — When `NavigationSection::Reports` is active, show a placeholder view:
    - Headline: "Reports" (`headline-lg`)
    - Message: "Report browsing will be available in a future release. Use 'Open Report Folder' from the completion screen to access generated reports."
    - This ensures navigation routing works for all 3 sections without requiring full Reports implementation.

### 13.E – Ingestion Worker Refactor for Structured Progress

- [ ] **13.14 – Refactor `start_ingestion_worker` to emit structured progress** — The current worker emits only 2 generic `Progress(String)` messages. Refactor to:
    1. Emit `ParsingStarted` with file metadata after opening workbook.
    2. Emit `ParsingLog` for each significant event (sheet opened, column mapped, warning, row milestone).
    3. Emit `ParsingProgress` with row counts at intervals (every 100 rows or per-sheet).
    4. This requires modifying `parse_excel_file` to accept a sender channel (`mpsc::Sender<WorkerMessage>`) or returning an iterator/callback pattern.

- [ ] **13.15 – Update `process_worker_messages` handler** — Extend the message processing loop to handle the new structured message variants (`ParsingStarted`, `ParsingLog`, `ParsingProgress`). Update the corresponding `NethericaApp` fields and ensure `ctx.request_repaint()` is called after each update.
