# Step 4 - Worker And Message Refactor

This file expands `ui_rework_task.md` Step 4 (`12.10` to `12.12`).

## Goal

Upgrade the UI worker pipeline so the app can render real parsing metadata, log output, and progress instead of a generic spinner.

## Current Anchors

- `src/ui/mod.rs:127` - current `WorkerMessage`
- `src/ui/mod.rs:265` - `start_ingestion_worker()`
- `src/ui/mod.rs:300` - `start_commit_worker()`
- `src/ui/mod.rs:452` - `process_worker_messages()`
- `src/ingestion.rs:68` - `prepare_ingestion_dry_run()`
- `src/ingestion.rs:250` - `ParsedWorkbook`
- `src/ingestion.rs:257` - `parse_excel_file()`
- `src/ingestion.rs:266` - `sheet_names = workbook.sheet_names().to_vec()`

## Hard Decision Already Made

Do not couple `src/ingestion.rs` directly to `ui::WorkerMessage`.

Instead, add an ingestion-side progress callback or ingestion-side event enum, then map those events to `WorkerMessage` inside `ui/worker.rs`.

This keeps the domain/parser code reusable and prevents `ingestion.rs` from depending on the UI module.

## Exact `WorkerMessage` Additions

Extend the existing enum with these variants:

```rust
ParsingStarted {
    filename: String,
    file_size: u64,
    sheet_count: usize,
    sheet_names: Vec<String>,
},
ParsingLog {
    timestamp: String,
    level: String,
    message: String,
},
ParsingProgress {
    current_sheet: String,
    rows_processed: usize,
    total_rows: usize,
},
DryRunTimingComplete {
    elapsed: std::time::Duration,
},
```

Keep the current `Progress(String)` variant. It is still useful for commit-side updates and fallback status text.

## Recommended Ingestion-Side Event Shape

Inside `src/ingestion.rs`, add a private event enum near `ParsedWorkbook`:

```rust
enum ParseProgressEvent {
    Started {
        filename: String,
        file_size: u64,
        sheet_count: usize,
        sheet_names: Vec<String>,
    },
    Log {
        level: &'static str,
        message: String,
    },
    Progress {
        current_sheet: String,
        rows_processed: usize,
        total_rows: usize,
    },
}
```

This enum should remain private to `ingestion.rs`.

## Recommended Parser Refactor

Do not break the public signature of `prepare_ingestion_dry_run()`.

Use this layered approach:

```rust
pub fn prepare_ingestion_dry_run(...) -> AppResult<PendingIngestionCommit> {
    prepare_ingestion_dry_run_with_events(..., |_| {})
}

fn prepare_ingestion_dry_run_with_events<F>(..., mut emit: F) -> AppResult<PendingIngestionCommit>
where
    F: FnMut(ParseProgressEvent),
{
    // same logic, but parser emits events
}
```

Then do the same for parsing:

```rust
fn parse_excel_file_with_events<F>(path: &Path, config: &Config, mut emit: F) -> AppResult<ParsedWorkbook>
where
    F: FnMut(ParseProgressEvent),
{
    // body of current parse_excel_file plus emits
}

fn parse_excel_file(path: &Path, config: &Config) -> AppResult<ParsedWorkbook> {
    parse_excel_file_with_events(path, config, |_| {})
}
```

This preserves existing tests and existing public call sites.

## What To Emit And When

### 1. After workbook open

Right after:

```rust
let mut workbook = open_workbook_auto(path)?;
let sheet_names = workbook.sheet_names().to_vec();
```

emit:

```rust
emit(ParseProgressEvent::Started {
    filename,
    file_size,
    sheet_count: sheet_names.len(),
    sheet_names: sheet_names.clone(),
});
```

Get `filename` from `path.file_name()` and `file_size` from filesystem metadata.

### 2. When a configured sheet starts

Before iterating its rows, emit a log and a progress baseline.

Example log text:

- `Opening sheet 'GAUZE-01'`
- `Scanning required columns for 'GAUZE-01'`

### 3. When required columns are found or missing

Emit `INFO` or `WARN` log events.

Example messages:

- `Mapped required columns for 'GAUZE-01'`
- `Required columns not found in 'GAUZE-01'; sheet skipped`

### 4. During row iteration

After each accepted row, or after every N rows if per-row updates are too noisy, emit:

```rust
emit(ParseProgressEvent::Progress {
    current_sheet: product.id.clone(),
    rows_processed,
    total_rows,
});
```

Use `range.height().saturating_sub(1)` for `total_rows`.

### 5. For warnings already logged through `tracing::warn!`

Also emit a `WARN` progress event with a user-facing string. Do not rely on tracing output to feed the UI.

### 6. After dry-run preparation finishes

In the UI worker, measure elapsed time around the dry-run call and emit:

```rust
WorkerMessage::DryRunTimingComplete { elapsed }
```

## UI Worker Mapping Rule

Inside `start_ingestion_worker()`, map ingestion-side events to `WorkerMessage`.

Example shape:

```rust
let dry_run_started = std::time::Instant::now();

let prepared = ingestion::prepare_ingestion_dry_run_with_events(
    &path,
    &config,
    &repository,
    |event| match event {
        ParseProgressEvent::Started { filename, file_size, sheet_count, sheet_names } => {
            let _ = tx.send(WorkerMessage::ParsingStarted { filename, file_size, sheet_count, sheet_names });
        }
        ParseProgressEvent::Log { level, message } => {
            let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
            let _ = tx.send(WorkerMessage::ParsingLog { timestamp, level: level.to_string(), message });
        }
        ParseProgressEvent::Progress { current_sheet, rows_processed, total_rows } => {
            let _ = tx.send(WorkerMessage::ParsingProgress { current_sheet, rows_processed, total_rows });
        }
    },
)?;

let _ = tx.send(WorkerMessage::DryRunTimingComplete {
    elapsed: dry_run_started.elapsed(),
});
```

The exact helper names can vary. The important part is the separation of responsibilities.

## `process_worker_messages()` Update Plan

Extend the current match with these new branches:

- `ParsingStarted`:
  - populate `parsing_file_metadata`
- `ParsingLog`:
  - push into `parsing_logs`
- `ParsingProgress`:
  - update `parsing_progress`
- `DryRunTimingComplete`:
  - store `dry_run_elapsed`

Keep the current behavior for:

- `DryRunData`
- `DryRunPrepared`
- `Completed`
- `Error`

Do not remove the call to `handle_report_ready()` on completion.

## Completion Snapshot Update

When handling `Completed(outcome)`, capture the completion snapshot before clearing anything important.

Use `pending_commit` if it is still available, or capture needed values earlier before `take()` if required.

Do not lose:

- filename
- file hash
- row count
- archive pending flag

## Recommended Work Order

1. Add the new `WorkerMessage` variants.
2. Add ingestion-side event enum and callback-based helper path.
3. Update `start_ingestion_worker()` to send structured UI events.
4. Add `DryRunTimingComplete` emission.
5. Update `process_worker_messages()`.
6. Run `cargo check`.
7. Run `cargo test`.

## Common Mistakes To Avoid

- Do not import `ui::WorkerMessage` into `src/ingestion.rs`.
- Do not remove `Progress(String)` yet.
- Do not break the existing public `prepare_ingestion_dry_run()` API if tests do not need that change.
- Do not drop the current error handling or completion side effects.

## Acceptance Criteria

- The UI has enough structured data to render file metadata, logs, and progress.
- Existing dry-run and completion behavior still works.
- `cargo check` and `cargo test` pass.
