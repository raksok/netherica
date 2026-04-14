# Step 5 - Local Design System Primitives

This file expands `ui_rework_task.md` Step 5 (`12.13` to `12.16`).

## Goal

Extract the recurring visual language into reusable theme constants and simple UI helpers so later view work becomes mostly composition rather than repeated styling decisions.

## Current Anchors

- `src/ui/mod.rs:42` - current inline `apply_design_system()`
- `src/ui/mod.rs:16` to `src/ui/mod.rs:40` - current font registration
- `src/ui/mod.rs:709` to `src/ui/mod.rs:721` - current toast
- `src/ui/mod.rs:723` to `src/ui/mod.rs:739` - current error modal
- `src/ui/mod.rs:741` to `src/ui/mod.rs:766` - current report-ready modal
- `gui_design/nordic_precision/DESIGN.md` - design rules
- `gui_design/*/code.html` - concrete palette and component examples

## Theme Constants To Define First

In `src/ui/theme.rs`, define at least these constants up front:

```rust
pub(crate) const SURFACE: egui::Color32 = egui::Color32::from_rgb(13, 19, 30);
pub(crate) const SURFACE_CONTAINER_LOWEST: egui::Color32 = egui::Color32::from_rgb(8, 14, 25);
pub(crate) const SURFACE_CONTAINER_LOW: egui::Color32 = egui::Color32::from_rgb(22, 28, 39);
pub(crate) const SURFACE_CONTAINER: egui::Color32 = egui::Color32::from_rgb(26, 32, 43);
pub(crate) const SURFACE_CONTAINER_HIGH: egui::Color32 = egui::Color32::from_rgb(36, 42, 54);
pub(crate) const SURFACE_CONTAINER_HIGHEST: egui::Color32 = egui::Color32::from_rgb(47, 53, 65);
pub(crate) const SURFACE_VARIANT: egui::Color32 = egui::Color32::from_rgb(47, 53, 65);
pub(crate) const SURFACE_BRIGHT: egui::Color32 = egui::Color32::from_rgb(51, 57, 69);

pub(crate) const PRIMARY: egui::Color32 = egui::Color32::from_rgb(163, 220, 236);
pub(crate) const PRIMARY_CONTAINER: egui::Color32 = egui::Color32::from_rgb(136, 192, 208);
pub(crate) const SECONDARY: egui::Color32 = egui::Color32::from_rgb(169, 202, 235);
pub(crate) const TERTIARY: egui::Color32 = egui::Color32::from_rgb(240, 207, 143);
pub(crate) const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 180, 171);

pub(crate) const ON_SURFACE: egui::Color32 = egui::Color32::from_rgb(221, 226, 242);
pub(crate) const ON_SURFACE_VARIANT: egui::Color32 = egui::Color32::from_rgb(192, 200, 203);
pub(crate) const ON_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0, 54, 64);
pub(crate) const OUTLINE: egui::Color32 = egui::Color32::from_rgb(138, 146, 149);
pub(crate) const OUTLINE_VARIANT: egui::Color32 = egui::Color32::from_rgb(64, 72, 75);
```

If later components need more tokens, add them. Do not keep magic RGB literals scattered through the views.

## Theme Rules To Follow

- Honor the `No-Line Rule` from `DESIGN.md`.
- Use tonal layering before adding strokes.
- Use shadows only for modal/floating surfaces.
- Prefer simple `egui::Frame` styling over custom painter code.

## Font Strategy

The design target is Inter + Thai fallback.

Implementation instructions:

1. Keep UTF-8 / Thai support working.
2. If Inter TTF files are added, register them as the primary proportional font.
3. Keep `asset/fonts/Sarabun/Sarabun-Regular.ttf` or `asset/fonts/NotoSansThaiLooped/NotoSansThaiLooped-Regular.ttf` as Thai-capable fallback.
4. Keep egui monospace fallback intact.

Recommended priority order if Inter is bundled:

```text
Proportional: Inter -> Sarabun/NotoSansThaiLooped -> default egui fallbacks
Monospace: default egui monospace -> Thai fallback appended if necessary
```

Do not remove fallback fonts from the family list.

## Simple Component API

Keep `components.rs` as free functions. Do not introduce stateful component structs.

Use signatures like these:

```rust
pub(crate) fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response
pub(crate) fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response
pub(crate) fn ghost_button(ui: &mut egui::Ui, text: &str) -> egui::Response

pub(crate) fn status_card(ui: &mut egui::Ui, label: &str, value: &str)
pub(crate) fn metric_card(ui: &mut egui::Ui, label: &str, value: &str)
```

If a helper needs richer content, add a second helper later. Do not over-generalize now.

## Button Style Rules

### Primary button

- fill: `PRIMARY_CONTAINER` or a left-to-right approximation of the primary gradient using the closest egui fill you can support cleanly
- text color: dark/on-primary tone
- rounding: 6 px
- weight: semibold or strong text

### Secondary button

- fill: `SURFACE_CONTAINER_HIGH` or `SURFACE_CONTAINER_HIGHEST`
- text color: `ON_SURFACE`
- rounding: 6 px

### Ghost button

- transparent fill
- primary or on-surface-variant text
- no heavy outline unless hovered/focused

## Card Style Rules

### Status card

- frame fill: `SURFACE_CONTAINER_LOW`
- rounding: 12 px
- label: small uppercase, `ON_SURFACE_VARIANT`
- value: regular body text, `ON_SURFACE`

### Metric card

- same base frame
- value: larger headline-sized text

## Overlay Rendering Plan

### Toast

Replace the current bottom-right popup with a top-right anchored card.

Use a helper similar to:

```rust
pub(crate) fn render_toast(ctx: &egui::Context, toast: &mut Option<(String, std::time::Instant)>) {
    // auto-dismiss after 5 seconds
    // explicit close button
}
```

Rules:

- anchor: top-right
- auto-dismiss: 5 seconds
- close button: required
- fill: `SURFACE_CONTAINER_HIGHEST`
- soft shadow allowed

### Error modal

Replace the plain `Window::new("Error")` feel with:

- full-screen dark translucent overlay
- centered card using `SURFACE_CONTAINER_HIGH`
- stronger title styling using `ERROR`

Suggested helper:

```rust
pub(crate) fn render_error_modal(ctx: &egui::Context, error: &mut Option<String>)
```

### Report-ready modal

Reuse the error-modal treatment, but with neutral/success styling.

It still needs these actions:

- `Open Report`
- `Open Report Folder`
- `Close`

Keep behavior unchanged; only restyle it.

## Recommended Work Order

1. Move theme constants into `theme.rs`.
2. Update `apply_design_system()` to use named constants.
3. Update font registration without regressing tests.
4. Add button helpers.
5. Add card helpers.
6. Restyle toast and modal helpers.
7. Run `cargo check` and `cargo test`.

## Common Mistakes To Avoid

- Do not leave raw RGB literals in every view.
- Do not build a highly abstract component system; simple helpers are enough.
- Do not break the font fallback test.
- Do not remove the existing modal/toast behaviors while restyling them.

## Acceptance Criteria

- Theme colors live in named constants.
- Buttons/cards have reusable helpers.
- Toast and modal surfaces follow the local design language.
- `cargo check` and `cargo test` pass.
