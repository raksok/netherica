# Step 8 - Cleanup And Consistency Pass

This file expands `ui_rework_task.md` Step 8 (`12.25` to `12.27`).

## Goal

Remove leftover placeholder styling and enforce a consistent visual system across all sections after the main view work is complete.

## Current Anchors To Audit First

These are the known line-heavy leftovers in the current baseline before the split:

- `src/ui/mod.rs:221` - `ui.group(...)`
- `src/ui/mod.rs:246` - `ui.group(...)`
- `src/ui/mod.rs:424` - `ui.separator()`
- `src/ui/mod.rs:828` - `ui.separator()`
- `src/ui/mod.rs:703` - stale `Inventory` nav label
- `src/ui/mod.rs:768` - stale in-content versioned heading
- `src/ui/mod.rs:774` - `Parsing file...`
- `src/ui/mod.rs:781` - `Committing to database...`
- `src/ui/mod.rs:785` - `Process complete!`

After refactoring, these exact line numbers will move, but the same kinds of leftovers must still be removed.

## 12.25 - Separator And Group Cleanup

### Search Targets

Run these searches after the main implementation is done:

- `ui.separator(`
- `ui.group(`

### Replacement Rule

- replace `ui.separator()` with spacing plus tonal separation where appropriate
- replace `ui.group()` with `egui::Frame` using the design-system surface colors

Preferred frame pattern:

```rust
egui::Frame::none()
    .fill(SURFACE_CONTAINER_LOW)
    .rounding(egui::Rounding::same(12.0))
    .inner_margin(egui::Margin::same(16.0))
    .show(ui, |ui| {
        // content
    });
```

Do not add visible 1 px borders unless there is a concrete readability need.

## 12.26 - Typography, Spacing, And Hover Audit

### Typography Rules

Apply a consistent scale. Use these values unless a view has a justified exception:

- page title: `32.0`
- section heading: `24.0`
- card title: `18.0` to `20.0`
- body: `14.0`
- small/body secondary: `12.0` to `13.0`
- micro label: `11.0` to `12.0`, uppercase when appropriate

### Text Color Rules

- primary text: `ON_SURFACE`
- secondary/supporting text: `ON_SURFACE_VARIANT`
- warnings: `TERTIARY`
- destructive/error emphasis: `ERROR`

### Spacing Rules

- prefer `ui.add_space(...)` over separators
- keep card padding consistent within each screen
- use generous breathing room instead of extra lines or boxes

### Hover Rules

- cards: shift from `SURFACE_CONTAINER_LOW` to `SURFACE_CONTAINER` or `SURFACE_CONTAINER_HIGHEST`
- buttons: modest fill/brightness shift only
- tables: row hover uses a slightly brighter surface, not a hard border

### Thai Text Check

Where Thai text is present in filenames or config data, verify:

- glyphs render correctly
- line height is acceptable
- fallback fonts do not collapse into boxes or missing glyphs

## 12.27 - Stale Copy And Shell Leftover Audit

### Search Targets

Run searches for these strings or their later equivalents:

- `Inventory`
- `Netherica v0.2.2 - Ingestion System`
- `Parsing file...`
- `Committing to database...`
- `Process complete!`
- `File Ingestion`
- `Configuration Summary`

### Rule

If the string is clearly placeholder copy from the old UI, replace it with the new screen-specific language.

Do not remove legitimate status or error messages just because they are plain text.

## Recommended Work Order

1. Search for separators and groups.
2. Replace them with frames and spacing.
3. Sweep typography sizes and text colors view by view.
4. Search for stale copy.
5. Run `cargo check`.

## Common Mistakes To Avoid

- Do not add borders while trying to remove separators.
- Do not use multiple different text scales for equivalent elements.
- Do not leave a mix of old placeholder copy and new design-system screens.

## Acceptance Criteria

- No obvious separator/group leftovers remain.
- Typography looks intentional and consistent.
- Stale placeholder copy is gone.
- `cargo check` passes.
