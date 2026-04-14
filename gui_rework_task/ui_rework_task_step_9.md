# Step 9 - Validation And Visual QA

This file expands `ui_rework_task.md` Step 9 (`12.28` to `12.31`).

## Goal

Verify the rework as an end-to-end UI delivery, not just as a set of compiling Rust files.

## Command Validation Order

Run the checks in this order so failures are easier to isolate:

1. `cargo check`
2. `cargo test`
3. `cargo clippy --all-targets --all-features -- -D warnings`
4. `cargo build --release`

If a command fails because of a UI change you made, fix it before continuing.

If a command fails for an unrelated pre-existing issue outside the UI work, stop and report that blocker instead of patching unrelated subsystems.

## 12.28 - Compilation And Lint Gate

### Minimum Success Bar

- all commands above succeed
- no new warnings in UI modules
- the app still launches

## 12.29 - Runtime Smoke Test: Full Ingestion Flow

Use a real `.xlsx` workbook supplied by the operator. No sample workbook is checked into this repo.

### Manual Script

1. Launch the app.
2. Confirm the window title is `Netherica | Pharmacy Reconciliation`.
3. Confirm the sidebar shows only `Ingestion`, `Reports`, and `Settings`.
4. In Idle view:
   - confirm the hero CTA renders
   - confirm last-run / sync / config cards render
5. Choose an `.xlsx` file with the file picker.
6. Confirm the progress screen shows:
   - metadata card
   - log stream
   - visible progress or indeterminate progress with live status
7. Let dry-run preparation finish.
8. Confirm the review screen shows:
   - summary metrics
   - styled table
   - confirm/cancel actions
9. If fallback warning is triggered, confirm the checkbox gate works.
10. Confirm the commit path transitions into the commit/in-flight view.
11. Confirm the completion screen shows:
   - success summary
   - report actions
   - archive retry action
   - new-file reset action

## 12.30 - Runtime Smoke Test: Navigation, Modal, And Warning Flows

### Navigation

1. Click `Reports` and confirm the reports landing page renders.
2. Click `Settings` and confirm the Departments view renders.
3. Switch to Products and confirm product cards render.
4. Return to Ingestion and confirm workflow state is preserved appropriately.

### Error Flow

Use a duplicate file or another known invalid input.

Confirm:

- the error modal appears
- the overlay darkens the background
- dismissing the modal works

### Toast Flow

Confirm:

- storage fallback toast still appears at most once when applicable
- warning toasts appear in the top-right
- the close button works
- auto-dismiss works after the configured timeout

### Report-Ready Flow

Confirm:

- `Open Report` works
- `Open Report Folder` works
- closing the modal works without side effects

## 12.31 - Visual QA Against Local References

Compare the implemented screens against the local `gui_design/*/screen.png` and `code.html` files.

### What To Compare

- overall shell proportions
- left rail / header / content hierarchy
- tone and density of cards
- table feel and hover states
- action-button emphasis
- use of tonal layering instead of borders
- spacing rhythm and text hierarchy

### Allowed Adaptations

These differences are allowed and expected:

- no `Inventory` nav item
- reports screen is custom because there is no local reports mock
- settings are read-only even though the mock shows editing affordances
- completion screen includes `Retry Archive`

### Final Review Questions

Before considering the work complete, answer these:

1. Does the UI look like one coherent product rather than a mix of old and new screens?
2. Are there any obvious old placeholder strings still visible?
3. Are any views still using line-heavy separators or groups?
4. Does Thai text render correctly where present?
5. Does every primary action still work?

## Acceptance Criteria

- Validation commands pass.
- Manual smoke tests pass.
- Visual QA is complete and documented in the final summary.
