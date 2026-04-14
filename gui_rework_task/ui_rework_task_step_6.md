# Step 6 - Ingestion Workflow Views

This file expands `ui_rework_task.md` Step 6 (`12.17` to `12.20`).

## Goal

Replace the current ingestion workflow placeholders with locally referenced layouts while preserving the real workflow behavior already implemented in the app.

## Current Anchors

- `src/ui/mod.rs:220` - current Idle view
- `src/ui/mod.rs:333` - current Dry Run view
- `src/ui/mod.rs:773` to `src/ui/mod.rs:783` - current Parsing and Committing placeholder views
- `src/ui/mod.rs:784` to `src/ui/mod.rs:823` - current Complete view actions
- `gui_design/ingestion_1idle/code.html`
- `gui_design/ingestion_3progress/code.html`
- `gui_design/ingestion_2review/code.html`
- `gui_design/ingestion_4complete/code.html`

## Important Compatibility Rules

- Preserve the real file-picking and ingestion workflow.
- Preserve the transaction-date fallback acknowledgement gate in the review step.
- Preserve report folder opening, report regeneration, and archive retry actions in the completion step.
- Do not blindly copy stale mock details such as old navigation labels.

## 12.17 - Idle View Implementation Guide

### Design Reference

Use `gui_design/ingestion_1idle` as the visual reference.

### Required Runtime Behavior

Keep these behaviors:

- file picker uses `rfd`
- selected file path is visible after choosing a file
- ingestion does not start unless the user explicitly starts it

### Recommended Layout

Use a 2-part top area and a 3-card bottom row.

#### Left hero card

Content:

- headline: `Ready for new reconciliation run?`
- body copy about uploading the latest Excel export
- primary button: `Select Excel File`
- after selection, show:
  - selected filename or path
  - secondary or primary action: `Start Ingestion`

Do not auto-start ingestion on file selection. Keep the current 2-step interaction.

#### Right summary card

Show configuration summary from the real config:

- product count: `self.config.products.len()`
- department count: `self.config.departments.len()`
- optional strict chronology status from `self.config.settings.strict_chronological`

#### Bottom row of status cards

Use three cards:

- `Last Run`
  - from `last_run_timestamp`
  - fallback text: `No files processed`
- `Sync State`
  - show DB connection and storage source
- `Config Status`
  - product and department counts

### Remove From The Current View

- the two `ui.group()` blocks
- the raw department list dump
- the raw database path label as a main content element

## 12.18 - Parsing And Committing View Implementation Guide

### Design Reference

Use `gui_design/ingestion_3progress` as the main reference.

### One Shared Shell, Two States

Use the same overall layout for both `AppState::Parsing` and `AppState::Committing`.

#### Parsing state

Required content:

- headline: `Analyzing Data Structure`
- subtext showing current sheet from `parsing_progress`
- file metadata card from `parsing_file_metadata`
- log console from `parsing_logs`
- progress bar using `rows_processed / total_rows`

If totals are missing, use an indeterminate/animated progress bar but still show logs and metadata.

#### Committing state

Reuse the same shell, but swap the copy:

- headline: `Finalizing Reconciliation`
- subtext from `status_message`
- keep the metadata card if available
- log area can show accumulated parsing logs plus a final commit status line
- no buttons except maybe disabled informational controls; user should not change course here

### Warning Coloring In Log Stream

- `INFO` rows: neutral/on-surface-variant
- `WARN` rows: `TERTIARY`
- `ERROR` rows if any: `ERROR`

### Remove From The Current View

- `ui.label("Parsing file...")`
- `ui.label("Committing to database...")`
- fixed `ProgressBar::new(0.5)` placeholders

## 12.19 - Dry Run Review Implementation Guide

### Design Reference

Use `gui_design/ingestion_2review`.

### Required Behavior To Preserve

- review table still comes from `self.dry_run_data`
- `Confirm` is disabled unless `can_confirm_commit()` is true
- transaction-date fallback acknowledgement remains visible and required when present
- `Cancel` returns to Idle and clears pending dry-run state

### Recommended Layout Sections

1. page title and short subtitle
2. compact stepper/progress bar if implemented
3. summary metric row
4. action/info strip
5. main table card
6. bottom confirm/cancel actions

### Summary Metrics

Use at least these:

- products / rows reviewed: `self.dry_run_data.len()`
- warnings: `compute_warning_count(&self.dry_run_data)`
- dry-run duration: `dry_run_elapsed`

### Table Rendering Rules

Continue using `egui_extras::TableBuilder`.

Restyle, do not replace.

- header fill: `SURFACE_CONTAINER_LOW`
- row hover: `SURFACE_CONTAINER_HIGH`
- no heavy grid lines
- keep current columns and current business data

### Fallback Warning Placement

If `pending.transaction_date_fallback_used` is true, show the warning block above the table or in the action strip. Keep the checkbox acknowledgment exactly functional.

## 12.20 - Completion View Implementation Guide

### Design Reference

Use `gui_design/ingestion_4complete`, but adapt it to the real product behavior.

### Required Runtime Behavior To Preserve

- `Open Report Folder`
- `Regenerate Last Report`
- `Retry Archive`
- `New File`

The mock does not show `Retry Archive`, but the app already supports it. Keep it.

### Recommended Layout

#### Main success card

Show:

- headline: `Process Completion`
- success message: `Reconciliation Successful`
- filename from `completed_filename`
- rows processed from `completed_rows_processed`
- optional archive status banner if `completed_archive_move_pending`

#### Action column or action row

Buttons:

- `Open Report Folder`
- `Regenerate Last Report`
- `Retry Archive`
- `New File`

`New File` should still reset to the Idle workflow.

#### Metadata section

Show:

- execution time from `pipeline_start`
- archive status from `completed_archive_move_pending`
- validator/app version from `APP_VERSION`
- truncated hash from `completed_file_hash`

If a metric is unknown, show a graceful fallback like `Unavailable` rather than leaving an empty field.

## Recommended Work Order

1. Implement the Idle view first.
2. Implement the shared Parsing/Committing shell.
3. Restyle the Dry Run review without changing its business logic.
4. Redesign the Completion view while preserving all 4 existing actions.
5. Run `cargo check`.

## Common Mistakes To Avoid

- Do not auto-start ingestion from the Idle view.
- Do not remove the fallback acknowledgment checkbox.
- Do not remove `Retry Archive` just because it is absent from the mock.
- Do not replace the dry-run table with a non-virtualized custom layout.

## Acceptance Criteria

- Idle, Parsing, Review, and Completion all follow the local design language.
- The real ingestion flow still works end to end.
- Existing business behaviors remain intact.
- `cargo check` passes.
