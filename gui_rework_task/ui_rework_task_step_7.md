# Step 7 - Reports And Settings Routing

This file expands `ui_rework_task.md` Step 7 (`12.21` to `12.24`).

## Goal

Implement the top-level non-ingestion sections so the shell feels complete and the app can navigate cleanly among Ingestion, Reports, and Settings.

## Current Anchors

- `src/config.rs:10` - `Config`
- `src/config.rs:16` - `products`
- `src/config.rs:17` - `departments`
- `src/config.rs:35` - `ProductConfig`
- `src/ui/mod.rs:529` - `handle_report_ready()`
- `src/ui/mod.rs:557` - `open_report_folder_action()`
- `src/ui/mod.rs:583` - `open_latest_report_action()`
- `gui_design/settings_departments/code.html`
- `gui_design/settings_products/code.html`

## Hard Decisions Already Made

- Settings is read-only in this phase.
- Do not implement create/edit/delete product or department flows.
- Reports has no dedicated local mock, so build it from the design system plus current app capabilities.

## 12.21 - Settings Tab Routing

### Exact Behavior

When `active_section == NavigationSection::Settings`:

- render a page header for Settings
- render a 2-tab switcher:
  - `Departments`
  - `Products`
- route content from `active_settings_tab`

Suggested shape:

```rust
impl NethericaApp {
    pub(crate) fn render_settings_view(&mut self, ui: &mut egui::Ui) {
        self.render_settings_tab_bar(ui);
        ui.add_space(16.0);

        match self.active_settings_tab {
            SettingsTab::Departments => self.render_departments_view(ui),
            SettingsTab::Products => self.render_products_view(ui),
        }
    }
}
```

Use pill buttons or small segmented buttons. Do not use a browser-style tab bar.

## 12.22 - Departments View

### Design Reference

Use `gui_design/settings_departments` as the visual guide, but remove edit/add affordances.

### Actual Data Source

Use `self.config.departments`.

Because it is a `BTreeMap<String, String>`, the display order is already stable and sorted by key.

### Exact Card Content

For each entry `(department_code, department_name)`:

- top micro-label: `Department Code`
- prominent code text: `department_code`
- second micro-label: `Mapped Display Name`
- prominent display name: `department_name`

Do not invent node counts, status chips, or analytics that are not backed by real data.

### Empty State

Not expected in valid config, but still handle it gracefully:

`No departments configured.`

### Recommended Rendering Pattern

Use a responsive wrap or grid-like layout with cards built from `egui::Frame`.

Do not add:

- `Add Department`
- overflow menus
- edit icons

## 12.23 - Products View

### Design Reference

Use `gui_design/settings_products` as the visual guide, but again remove edit/add affordances.

### Actual Data Source

Use `self.config.products` in the order it is defined in config.

### Exact Card Content

For each `ProductConfig`:

- product ID
- display name
- unit
- subunit
- factor
- track_subunits

Recommended detail layout:

```text
ID: GAUZE-01
Sterile Gauze Pads
Unit: pack
Subunit: sheet
Factor: 12
Track subunits: true
```

### Formatting Rules

- show `factor` via `to_string()` unless there is already a shared decimal formatter
- show `track_subunits` as `Yes` / `No` or `Enabled` / `Disabled`

### Empty State

Not expected in valid config, but handle it gracefully:

`No products configured.`

## 12.24 - Reports Landing View

### There Is No Local Mock

Build a calm, design-system-consistent page rather than a placeholder sentence floating in empty space.

### Required Content

At minimum show:

- headline: `Reports`
- body copy describing current scope
- one informational card explaining that browsing/history UI is future work
- action area for currently supported report actions

### Recommended Actions

Use the existing app methods.

- `Open Latest Report`
  - enabled only if `last_report_path.is_some()`
  - calls `open_latest_report_action()`
- `Open Report Folder`
  - always available
  - calls `open_report_folder_action()`
- optional `Regenerate Last Report`
  - if you surface it here, reuse `regenerate_last_report()`

### Optional Context Card

If `last_report_path` exists, show a small card with:

- report filename
- parent folder path
- note that it opens in the system default browser/app

Do not query extra DB/report metadata in this step unless it is trivial.

## Recommended Work Order

1. Implement the settings tab bar.
2. Implement `render_departments_view()` from `config.departments`.
3. Implement `render_products_view()` from `config.products`.
4. Implement the reports landing view with real action buttons.
5. Run `cargo check`.

## Common Mistakes To Avoid

- Do not add editing workflows.
- Do not add fake status values copied from HTML mock data.
- Do not leave Reports as only a single text label if the existing app already has useful report actions.

## Acceptance Criteria

- Settings routes cleanly between Departments and Products.
- Departments and Products both render real config data.
- Reports renders a proper landing page with current report actions.
- `cargo check` passes.
