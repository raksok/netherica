# Step 3 - Runtime Summary State For Idle And Completion Views

This file expands `ui_rework_task.md` Step 3 (`12.6` to `12.9`).

## Goal

Add the missing app state needed to render the redesigned Idle, Progress, Review, and Completion screens using real data instead of placeholders.

## Current Anchors

- `src/ui/mod.rs:135` - current `NethericaApp` fields
- `src/ui/mod.rs:152` - `from_config()` defaults
- `src/ui/mod.rs:170` - `new()` startup behavior
- `src/repository.rs:48` - `get_max_transaction_date()`
- `src/storage.rs:6` - `DataRootSource`
- `src/storage.rs:12` - `DataDirectory`
- `src/ingestion.rs:30` - `IngestionOutcome`
- `src/ingestion.rs:52` - `PendingIngestionCommit`
- `src/domain.rs:5` - `DryRunRow`

## Exact New Types

Add this near the UI state definitions:

```rust
#[derive(Debug, Clone)]
pub struct ParsingFileMetadata {
    pub filename: String,
    pub file_size: u64,
    pub sheet_count: usize,
    pub sheet_names: Vec<String>,
}
```

## Exact New Fields On `NethericaApp`

Add these fields:

```rust
pub last_run_timestamp: Option<DateTime<Utc>>,
pub db_connected: bool,
pub storage_source: Option<DataRootSource>,

pub parsing_logs: Vec<(String, String, String)>,
pub parsing_file_metadata: Option<ParsingFileMetadata>,
pub parsing_progress: Option<(String, usize, usize)>,

pub completed_rows_processed: usize,
pub completed_filename: String,
pub completed_file_hash: String,
pub completed_archive_move_pending: bool,

pub pipeline_start: Option<std::time::Instant>,
pub dry_run_elapsed: Option<std::time::Duration>,
```

Keep the existing fields such as `selected_file`, `dry_run_data`, `pending_commit`, `last_report_path`, and `post_generation_guidance`.

## Default Values

Initialize them in `from_config()` like this:

```rust
last_run_timestamp: None,
db_connected: false,
storage_source: None,

parsing_logs: Vec::new(),
parsing_file_metadata: None,
parsing_progress: None,

completed_rows_processed: 0,
completed_filename: String::new(),
completed_file_hash: String::new(),
completed_archive_move_pending: false,

pipeline_start: None,
dry_run_elapsed: None,
```

## Startup Data Load In `new()`

Extend `NethericaApp::new()` with two non-fatal startup probes.

### Probe 1: Storage

Keep the current `DataDirectory::resolve()` behavior and add:

```rust
if let Ok(data_dir) = DataDirectory::resolve() {
    app.storage_source = Some(data_dir.root_source);
    app.maybe_show_storage_fallback_warning(&data_dir);
}
```

### Probe 2: Database + Last Run Timestamp

Do not crash if the DB cannot be opened.

```rust
match Database::new(&app.config.database_path) {
    Ok(db) => {
        let repository = Repository::new(&db);
        app.db_connected = true;
        app.last_run_timestamp = repository.get_max_transaction_date().ok().flatten();
    }
    Err(_) => {
        app.db_connected = false;
        app.last_run_timestamp = None;
    }
}
```

This is enough for the Idle cards.

## Completion Snapshot Rules

Before pending data is cleared, capture these UI values:

- rows processed: `pending.ledger_entries.len()`
- filename: `pending.filename.clone()`
- file hash: `pending.file_hash.clone()`
- archive pending: `outcome.archive_move_pending`

Do not try to re-derive these later from the database.

## Recommended Helper Methods

Add these helpers now so later steps stay simple.

### 1. Clear transient parsing state

```rust
fn clear_parsing_state(&mut self) {
    self.parsing_logs.clear();
    self.parsing_file_metadata = None;
    self.parsing_progress = None;
}
```

### 2. Clear completion snapshot for a new run

```rust
fn clear_completion_state(&mut self) {
    self.completed_rows_processed = 0;
    self.completed_filename.clear();
    self.completed_file_hash.clear();
    self.completed_archive_move_pending = false;
    self.pipeline_start = None;
    self.dry_run_elapsed = None;
}
```

### 3. Optional formatting helpers

These can live in `views/dry_run.rs`, `views/complete.rs`, or `components.rs` later:

```rust
pub(crate) fn compute_warning_count(rows: &[DryRunRow]) -> usize {
    rows.iter().filter(|row| row.closing_leftover < rust_decimal::Decimal::ZERO).count()
}

pub(crate) fn truncate_hash(hash: &str) -> String {
    if hash.len() <= 12 {
        hash.to_string()
    } else {
        format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
    }
}
```

If you add a duration formatter, keep it short and UI-oriented, for example `14.2s` or `1m 04s`.

## State-Transition Rules

- When a new ingestion starts:
  - set `pipeline_start = Some(Instant::now())`
  - clear old parsing state
  - clear old completion state
- When returning to Idle with `New File`:
  - clear parsing state
  - clear completion state
  - clear `dry_run_data`
  - clear `pending_commit`
- Do not clear `last_report_path` unless the UX explicitly requires it later.

## Recommended Work Order

1. Add `ParsingFileMetadata`.
2. Add all new app fields and defaults.
3. Extend `new()` with storage + database probes.
4. Add state-clear helpers.
5. Add warning-count and hash-format helpers.
6. Run `cargo check`.

## Acceptance Criteria

- The app can expose last-run time, DB status, and storage source.
- The app has structured state for progress/log metadata.
- The app has completion snapshot fields ready for the redesigned success screen.
- `cargo check` passes.
