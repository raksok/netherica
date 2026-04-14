# Step 1 - Preserve Current Working Behavior Before Restructure

This file expands `ui_rework_task.md` Step 1 (`12.1` to `12.2`) into an execution guide for a lower-capability implementation agent.

## Goal

Split `src/ui/mod.rs` into smaller files without changing runtime behavior.

The only acceptable differences in this step are file boundaries and internal organization. The app should still behave exactly like it does today.

## Current Anchors

- `src/ui/mod.rs:119` - `AppState`
- `src/ui/mod.rs:127` - `WorkerMessage`
- `src/ui/mod.rs:135` - `NethericaApp`
- `src/ui/mod.rs:183` - `maybe_show_storage_fallback_warning()`
- `src/ui/mod.rs:220` - `render_idle_view()`
- `src/ui/mod.rs:333` - `render_dry_run_view()`
- `src/ui/mod.rs:452` - `process_worker_messages()`
- `src/ui/mod.rs:509` - `regenerate_last_report()`
- `src/ui/mod.rs:529` - `handle_report_ready()`
- `src/ui/mod.rs:557` - `open_report_folder_action()`
- `src/ui/mod.rs:583` - `open_latest_report_action()`
- `src/ui/mod.rs:685` - `impl eframe::App for NethericaApp`
- `src/ui/mod.rs:839` onward - the current 8 UI tests

## Mandatory Constraints

- Do not change app behavior in this step.
- Do not rename user-visible states, button labels, or messages yet.
- Do not add new UI state fields yet unless the code will not compile without small temporary scaffolding.
- Keep all existing tests passing.

## Target File Layout

Create this structure:

```text
src/ui/
  mod.rs
  theme.rs
  sidebar.rs
  components.rs
  worker.rs
  views/
    mod.rs
    idle.rs
    progress.rs
    dry_run.rs
    complete.rs
    settings.rs
    reports.rs
```

`settings.rs`, `reports.rs`, and `components.rs` can be mostly stubs in this step if they are not used yet, but the module layout should exist so later steps have a predictable home.

## Recommended Module Ownership

Use this exact split unless there is a compile blocker.

### `src/ui/mod.rs`

Keep these items in `mod.rs`:

- `AppState`
- `NethericaApp`
- `OpenTarget`
- `build_print_guidance_message()`
- `resolve_report_folder()`
- `open_path_in_default_app()`
- `NethericaApp::from_config()`
- `NethericaApp::new()`
- `NethericaApp::maybe_show_storage_fallback_warning()`
- `NethericaApp::can_confirm_commit()`
- `NethericaApp::handle_dry_run_prepared()`
- `NethericaApp::process_worker_messages()`
- `NethericaApp::regenerate_last_report()`
- `NethericaApp::handle_report_ready()`
- `NethericaApp::open_report_folder_action()`
- `NethericaApp::open_latest_report_action()`
- the top-level `eframe::App` implementation
- tests that are still naturally about app state or helper functions

### `src/ui/theme.rs`

Move these items here:

- `UI_THAI_FONT_NAME`
- `UI_THAI_FONT_BYTES`
- `build_font_definitions_with_utf8_support()`
- `configure_egui_fonts()`
- `apply_design_system()`
- the font-related test

### `src/ui/worker.rs`

Move these items here:

- `WorkerMessage`
- `NethericaApp::start_ingestion_worker()`
- `NethericaApp::start_commit_worker()`

Keep these as `impl NethericaApp` methods inside `worker.rs` instead of inventing a new helper type.

### `src/ui/sidebar.rs`

Create one method here now even if it is still simple:

```rust
impl NethericaApp {
    pub(crate) fn render_sidebar(&mut self, ctx: &egui::Context) {
        // move current SidePanel code here first
    }
}
```

For this step, the method can render the exact current sidebar including the stale `Inventory` item. Later steps will redesign it.

### `src/ui/views/idle.rs`

Move `render_idle_view()` here as an `impl NethericaApp` method.

### `src/ui/views/dry_run.rs`

Move `render_dry_run_view()` here as an `impl NethericaApp` method.

### `src/ui/views/progress.rs`

Create helper methods for the current placeholder in-flight states so the `update()` method stays small:

```rust
impl NethericaApp {
    pub(crate) fn render_parsing_view(&mut self, ui: &mut egui::Ui) {
        ui.label("Parsing file...");
        ui.add(egui::ProgressBar::new(0.5).animate(true));
    }

    pub(crate) fn render_committing_view(&mut self, ui: &mut egui::Ui) {
        ui.label("Committing to database...");
        ui.add(egui::ProgressBar::new(0.5).animate(true));
    }
}
```

Do not redesign these views yet.

### `src/ui/views/complete.rs`

Move the current `AppState::Complete` rendering body into:

```rust
impl NethericaApp {
    pub(crate) fn render_complete_view(&mut self, ui: &mut egui::Ui) {
        // current complete-state buttons and actions
    }
}
```

### `src/ui/views/settings.rs` and `src/ui/views/reports.rs`

Create minimal placeholders only:

```rust
impl NethericaApp {
    pub(crate) fn render_settings_view(&mut self, ui: &mut egui::Ui) {
        ui.label("Settings placeholder");
    }

    pub(crate) fn render_reports_view(&mut self, ui: &mut egui::Ui) {
        ui.label("Reports placeholder");
    }
}
```

These are temporary compile helpers for later steps.

## Exact Mod Declarations

At the top of `src/ui/mod.rs`, use something close to this:

```rust
mod components;
mod sidebar;
mod theme;
mod worker;
mod views;
```

Inside `src/ui/views/mod.rs`:

```rust
pub(crate) mod complete;
pub(crate) mod dry_run;
pub(crate) mod idle;
pub(crate) mod progress;
pub(crate) mod reports;
pub(crate) mod settings;
```

Do not overuse `pub`. Prefer `pub(crate)` or private items unless another module truly needs access.

## How To Split the Methods Safely

Use separate `impl NethericaApp` blocks in different files. This is the least risky option.

Example:

```rust
// src/ui/views/idle.rs
use super::NethericaApp;
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_idle_view(&mut self, ui: &mut egui::Ui) {
        // moved body
    }
}
```

This avoids creating extra wrapper traits or changing call sites.

## Test Migration Plan

The UI currently has 8 tests. Move them like this.

### Keep in `src/ui/mod.rs`

- `fallback_prepared_payload_requires_ack_before_confirm`
- `non_fallback_prepared_payload_can_confirm_immediately`
- `print_guidance_contains_path_and_ctrl_p_instruction`
- `resolve_report_folder_prefers_latest_report_parent`
- `resolve_report_folder_falls_back_to_configured_reports_dir`
- `storage_fallback_warning_is_shown_only_once`
- `storage_executable_root_does_not_show_fallback_warning`

### Move to `src/ui/theme.rs`

- `font_definitions_register_noto_sans_thai_looped_with_fallbacks_preserved`

## Recommended Work Order

1. Create the new files with minimal imports and empty `impl NethericaApp` blocks.
2. Move the theme/font code first.
3. Move worker methods and `WorkerMessage`.
4. Move idle view, progress helpers, dry-run view, and complete view.
5. Move sidebar rendering into `render_sidebar()`.
6. Update `update()` in `mod.rs` to call the extracted methods.
7. Move the font test to `theme.rs`.
8. Run `cargo check`.
9. Fix imports and visibility.
10. Run `cargo test`.

## Common Mistakes To Avoid

- Do not accidentally delete the report-related action handlers.
- Do not move `OpenTarget` into `worker.rs` or a view module; it is a generic helper for report/file opening.
- Do not create circular imports between `mod.rs` and view modules.
- Do not replace the current tests with new tests. Move them; do not weaken coverage.

## Acceptance Criteria

- `src/ui/mod.rs` becomes an orchestrator instead of a 900+ line monolith.
- `cargo check` passes.
- `cargo test` passes.
- App behavior is unchanged from the pre-split baseline.
