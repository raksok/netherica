
## Phase 12: UI Upgrade — Nordic Precision (Editorial Calm)

> **Source of truth:**
> - Stitch MCP project `netherica design` (ID `18330809547273064391`) — 7 canonical screens
> - `DESIGN.md` — Full design tokens, component specs, do's/don'ts
> - `Netherica_rqrmnt.md` section 7 — View-by-view UI specification
>
> **Goal:** Transform the current functional-but-minimal `src/ui/mod.rs` into a polished, design-system-compliant UI matching the Stitch prototypes.

### 12.A – Module Decomposition & Foundation

- [ ] **12.1 – Split `ui/mod.rs` into sub-modules** — Extract from the current monolithic 985-line `src/ui/mod.rs` into focused files:
    - `ui/mod.rs` (re-exports, `NethericaApp` struct, `eframe::App` impl with top-level layout)
    - `ui/theme.rs` (all design system tokens, `apply_design_system()`, font setup)
    - `ui/sidebar.rs` (sidebar panel rendering with nav state)
    - `ui/views/idle.rs` (Idle view)
    - `ui/views/parsing.rs` (Parsing view)
    - `ui/views/dry_run.rs` (Dry Run / Review view)
    - `ui/views/complete.rs` (Completion view)
    - `ui/views/settings.rs` (Settings: Departments + Products sub-views)
    - `ui/components.rs` (reusable: `PrimaryButton`, `SecondaryButton`, `GhostButton`, `StatusCard`, `MetricCard`, toast overlay, modal overlay)
    - `ui/worker.rs` (`WorkerMessage` enum, ingestion/commit worker spawn functions)

- [ ] **12.2 – Introduce `NavigationSection` enum** — Add `enum NavigationSection { Ingestion, Reports, Settings }` (3 items per Stitch update, no Inventory). Wire active section into `NethericaApp` state so sidebar and central panel respect it. Default to `Ingestion`.

### 12.B – Design System Primitives (`ui/theme.rs` + `ui/components.rs`)

- [ ] **12.3 – Define all color token constants** — Create typed `const` values for every token from `DESIGN.md` Section 2 (surfaces, primaries, secondaries, tertiaries, error/Aurora, on-colors, outline-variant, inverse tokens). Current code only defines ~10 tokens; target is full palette (~40 tokens).

- [ ] **12.4 – Implement button variants** — Create helper functions or wrapper structs for three button styles from `DESIGN.md` 6.1:
    - `PrimaryButton`: gradient fill (`#a3dcec` to `#88c0d0`), `on-primary` text, `radius-md`. Since egui doesn't natively support gradients, use `primary-container` as solid fill with `on-primary` text as the faithful approximation.
    - `SecondaryButton`: `surface-container-highest` fill, `on-surface` text, `radius-md`.
    - `GhostButton`: transparent fill, `primary` text, `radius-md`.

- [ ] **12.5 – Implement `StatusCard` and `MetricCard` components** — Reusable card widgets per `DESIGN.md` 6.4:
    - `StatusCard`: `surface-container-low` background on `surface` parent, `radius-xl` (12px), label + value layout, hover shifts to `surface-container-highest` + ghost border at 30%.
    - `MetricCard`: variant for numeric metrics with `headline-lg` value + `label-md` title.

- [ ] **12.6 – Upgrade toast notification system** — Restyle per `Netherica_rqrmnt.md` 7.5:
    - Move from bottom-right to **top-right** anchor.
    - Increase auto-dismiss from 3s to **5s**.
    - Style: `surface-container-highest` fill, `radius-xl`, ambient shadow. (egui cannot do `backdrop-filter`, so use solid fill with shadow as approximation.)
    - Add dismiss button (x) for manual close.

- [ ] **12.7 – Upgrade error modal system** — Restyle per `Netherica_rqrmnt.md` 7.5:
    - Background: semi-transparent dark overlay (`surface-variant` at 70% alpha, rendered via `egui::Area` fill).
    - Modal card: `surface-container-high` background, `radius-xl`, ambient shadow (`0 24px 48px rgba(0,0,0,0.4)`).
    - Use `error` color (`#ffb4ab`) for error icon/title, `on-surface` for body text.
    - Buttons: use `PrimaryButton` for acknowledge, `SecondaryButton` for retry.

### 12.C – Sidebar Redesign (`ui/sidebar.rs`)

- [ ] **12.8 – Redesign sidebar panel** — Implement per `Netherica_rqrmnt.md` 7.2:
    - Background: `surface-container-low` (`#161c27`).
    - Logo section: "Netherica" in `headline-md` (24px semibold, `on-surface`), "Pharmacy Reconciliation" in `body-sm` (13px, `on-surface-variant`).
    - Navigation items (3): Ingestion, Reports, Settings.
    - Active item: `primary` (`#a3dcec`) text + `surface-container` background fill.
    - Inactive items: `on-surface-variant` (`#c0c8cb`) text + no background.
    - Remove the "Inventory" nav item (was dropped in Stitch update).
    - Footer: "Precision Reconciliation Engine" in `label-sm` (`on-surface-variant`), bottom-pinned.

### 12.D – Ingestion Views Redesign

- [ ] **12.9 – Redesign Idle view** — Implement per `Netherica_rqrmnt.md` 7.3.1 and Stitch screen `fcc6181c`:
    - Replace `ui.group` with proper layout.
    - Headline: "Ready for new reconciliation run?" (`headline-lg`, `on-surface`).
    - Description: "Upload your latest Pharmacy Excel export to begin the automated discrepancy audit." (`body-md`, `on-surface-variant`).
    - File upload button: `PrimaryButton` style ("Select Excel File").
    - Status Cards row (3x `StatusCard`): Last Run timestamp, Sync State, Config Status (product/department count).
    - Remove raw config dump; show clean summary only.

- [ ] **12.10 – Redesign Parsing view** — Implement per `Netherica_rqrmnt.md` 7.3.2 and Stitch screen `ba375e7c`:
    - Headline: "Analyzing Data Structure" (`headline-lg`).
    - Sub-headline: "Parsing sheet: {name}..." (`body-md`, `on-surface-variant`).
    - File Metadata Card (`surface-container` fill): filename, file size, sheet list.
    - Live log area: `ScrollArea` with `surface-container-lowest` background, monospace font, timestamp-prefixed entries. Warnings in `tertiary` color.
    - Progress bar: custom painted or standard `ProgressBar` with `primary` color fill on `surface-container-highest` track.
    - Wire `WorkerMessage::Progress` to populate log entries (store as `Vec<String>` in app state).

- [ ] **12.11 – Redesign Dry Run (Review) view** — Implement per `Netherica_rqrmnt.md` 7.3.3 and Stitch screen `1f6aa1e1`:
    - Headline: "Dry Run Review" (`headline-lg`).
    - Description: "Validate reconciliation metrics before generating the final ledger report." (`body-md`, `on-surface-variant`).
    - Summary Metric Cards row (3x `MetricCard`): product count / warning count / computation time.
    - Table: keep existing `TableBuilder` but restyle:
        - Header: `surface-container-low` background, text in `label-sm` uppercase `on-surface-variant`.
        - Rows: 24px height (1.5rem), no internal grid lines, hover to `surface-container-high`.
        - Ghost borders at 15% opacity using `outline-variant`.
    - Row count label: "Showing {n} of {total} reconciled products".
    - Confirmation footer on `surface-container-low` strip:
        - Warning: "Confirming will finalize the period-end reconciliation and update the central inventory ledger." (`body-sm`, `on-surface-variant`).
        - `GhostButton` "Cancel" + `PrimaryButton` "Confirm & Generate Report" (replaces current plain buttons).

- [ ] **12.12 – Redesign Completion view** — Implement per `Netherica_rqrmnt.md` 7.3.4 and Stitch screen `78a279e9`:
    - Headline: "Process Completion" (`headline-lg`).
    - Success banner: "Reconciliation Successful" (`headline-md`, `primary`).
    - Description: "Report successfully generated for {filename}..." (`body-md`, `on-surface-variant`).
    - Summary metrics inline: Rows Processed (`MetricCard`), Discrepancies Resolved (`MetricCard`).
    - Action cards (2x `StatusCard`):
        - "Open Report Folder" (`SecondaryButton`) — "Access generated artifacts".
        - "Regenerate Last Report" (`SecondaryButton`) — "Update with cached data".
    - New Cycle CTA: `PrimaryButton` "New File" — resets to Idle.
    - System Health & Metadata footer: 4 label-value pairs (`label-md` uppercase) — Execution Time, Data Integrity, Validator Version, Log Hash. Use `surface-container-low` strip.
    - Footer text: "Precision Reconciliation Engine" (`label-sm`, `on-surface-variant`).

### 12.E – Settings Views (New Feature)

- [ ] **12.13 – Implement Settings navigation routing** — When `NavigationSection::Settings` is active, show Settings sub-navigation (tab bar or pills) with two tabs: "Departments" and "Products". Default to Departments. Store active settings tab in `NethericaApp` state.

- [ ] **12.14 – Implement Settings: Departments view** — Implement per `Netherica_rqrmnt.md` 7.4.1 and Stitch screen `746808a3`:
    - Headline: "Department Configuration" (`headline-lg`).
    - Section title: "Department Registry" (`headline-md`).
    - Description: "Manage departmental taxonomy and display mapping." (`body-md`, `on-surface-variant`).
    - Card grid: one card per department from `config.departments`, `surface-container-low` fill, `radius-xl`. Code in `label-md` uppercase monospace, display name in `headline-sm`.
    - Read-only in v1 (no live CRUD to `config.toml`); display current config state.

- [ ] **12.15 – Implement Settings: Products view** — Implement per `Netherica_rqrmnt.md` 7.4.2 and Stitch screen `2a3ea4a0`:
    - Headline: "Product Configuration" (`headline-lg`).
    - Section title: "Inventory Ledger" (`headline-md`).
    - Description: "Configure medical supplies, reconciliation factors, and tracking units." (`body-md`, `on-surface-variant`).
    - Card grid: one card per product from `config.products`, `surface-container-low` fill, `radius-xl`. ID in `label-md` monospace, display name in `headline-sm`, details (unit, subunit, factor, track_subunits) in `body-sm`.
    - System Status footer: product count, config file path.
    - Read-only in v1; display current config state.

### 12.F – Polish & Compliance

- [ ] **12.16 – Remove all `ui.separator()` calls** — Audit the entire UI codebase and replace every `ui.separator()` with `ui.add_space()`. This enforces the No-Line Rule from `DESIGN.md`.

- [ ] **12.17 – Replace all `ui.group()` with `egui::Frame`** — Current code uses `ui.group()` which draws a bordered rectangle. Replace with `egui::Frame` configured with `surface-container-low` fill, `radius-xl` rounding, and no stroke (No-Line Rule).

- [ ] **12.18 – Apply typography hierarchy** — Audit heading/label usage across all views:
    - `headline-lg` (32px): page titles — use `RichText::new(...).size(32.0).strong()`.
    - `headline-md` (24px): section headings — use `RichText::new(...).size(24.0)`.
    - `body-md` (14px): primary text — default egui body.
    - `body-sm` (13px): secondary text — `RichText::new(...).size(13.0)`.
    - `label-md` (12px): card labels, wizard steps — `RichText::new(...).size(12.0).strong()`.
    - Apply `on-surface` for primary text, `on-surface-variant` for secondary/descriptions.

- [ ] **12.19 – Footer and status bar redesign** — Replace the current bottom separator + status text line with:
    - Remove `ui.separator()` at line 823.
    - Use `egui::TopBottomPanel::bottom` with `surface-container-low` fill.
    - Left: "Precision Reconciliation Engine" in `label-sm`.
    - Right: current state badge using `label-sm` styled text.

- [ ] **12.20 – Visual QA pass** — Run the application, visually compare each view against the Stitch screenshots (downloadable from the Stitch MCP screen entries), and fix any color mismatches, spacing inconsistencies, or design-system violations. Verify:
    - No borders visible (No-Line Rule).
    - All backgrounds use surface token hierarchy.
    - Hover states shift to `surface-container-high`.
    - Buttons use correct variant styles.
    - Toast/modals positioned and styled correctly.
    - Text hierarchy is visually distinguishable.
