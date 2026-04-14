# Step 2 - App Shell, Navigation, and Bootstrap

This file expands `ui_rework_task.md` Step 2 (`12.3` to `12.5`).

## Goal

Introduce real top-level navigation and modernize the app shell/window bootstrap without changing the underlying ingestion workflow logic.

## Current Anchors

- `src/main.rs:24` to `src/main.rs:31` - current `NativeOptions` and `run_native()` title
- `src/ui/mod.rs:693` to `src/ui/mod.rs:705` - current sidebar render logic
- `src/ui/mod.rs:768` - current in-content title `Netherica v0.2.2 - Ingestion System`

## Decisions Already Made

Do not re-decide these.

- Navigation sections are exactly:
  - `Ingestion`
  - `Reports`
  - `Settings`
- Settings tabs are exactly:
  - `Departments`
  - `Products`
- The stale `Inventory` nav item must be removed.
- The window title should be `Netherica | Pharmacy Reconciliation`.
- The crate version stays `0.2.2` in `Cargo.toml`.

## Exact New Types and Fields

Add these near the existing app state definitions:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationSection {
    Ingestion,
    Reports,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Departments,
    Products,
}
```

Add these fields to `NethericaApp`:

```rust
pub active_section: NavigationSection,
pub active_settings_tab: SettingsTab,
```

Default values in `from_config()`:

```rust
active_section: NavigationSection::Ingestion,
active_settings_tab: SettingsTab::Departments,
```

## `main.rs` Changes

Add this public constant near the top level of `src/main.rs`:

```rust
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
```

Update the window bootstrap to this shape:

```rust
let native_options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_title("Netherica | Pharmacy Reconciliation")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([960.0, 600.0]),
    ..Default::default()
};
```

Use the same string in `run_native()` for consistency.

## Recommended Shell Structure

Keep the shell simple and predictable.

```rust
self.render_sidebar(ctx);

egui::TopBottomPanel::bottom("status_bar")
    .resizable(false)
    .show(ctx, |ui| {
        self.render_status_bar(ui);
    });

egui::CentralPanel::default().show(ctx, |ui| {
    self.render_global_overlays(ctx);
    self.render_active_section(ui);
});
```

You do not need to use these exact helper names, but keep the responsibilities separated like this.

## Navigation Routing Rule

`AppState` is only for the ingestion workflow.

Top-level routing must first switch on `active_section`, then `Ingestion` can switch on `AppState`.

Use this structure:

```rust
match self.active_section {
    NavigationSection::Ingestion => self.render_ingestion_section(ui),
    NavigationSection::Reports => self.render_reports_view(ui),
    NavigationSection::Settings => self.render_settings_view(ui),
}
```

Inside `render_ingestion_section()`:

```rust
match self.state {
    AppState::Idle => self.render_idle_view(ui),
    AppState::Parsing => self.render_parsing_view(ui),
    AppState::DryRun => self.render_dry_run_view(ui),
    AppState::Committing => self.render_committing_view(ui),
    AppState::Complete => self.render_complete_view(ui),
}
```

## Sidebar Implementation Notes

For this step, the sidebar can still use `selectable_label()` if that keeps the change small. Later steps will restyle it.

Behavior rules:

- Clicking `Ingestion` sets `active_section = NavigationSection::Ingestion`
- Clicking `Reports` sets `active_section = NavigationSection::Reports`
- Clicking `Settings` sets `active_section = NavigationSection::Settings`
- Do not change `AppState` just because the top-level section changed

## Status Bar Requirements

Replace the bottom inline separator + labels with a dedicated bottom panel.

Minimum content:

- left side: `Precision Reconciliation Engine`
- right side: current `AppState` plus `status_message`

Do not overdesign it yet. The important part is moving to a stable shell structure.

## Remove These Current Shell Artifacts

- `Inventory` nav item
- duplicated in-content title: `Netherica v0.2.2 - Ingestion System`

## Recommended Work Order

1. Add `NavigationSection` and `SettingsTab`.
2. Add new fields to `NethericaApp` and default them.
3. Update `main.rs` title and viewport sizing.
4. Extract a `render_ingestion_section()` helper.
5. Add section routing in `update()`.
6. Update the sidebar to switch `active_section`.
7. Add a simple bottom status bar panel.
8. Run `cargo check`.

## Acceptance Criteria

- The shell routes among `Ingestion`, `Reports`, and `Settings`.
- No `Inventory` navigation remains.
- The app title is `Netherica | Pharmacy Reconciliation`.
- Window defaults are larger than the current `800x600` shell.
- `cargo check` passes.
