# Coder Handoff

This document is the handoff wrapper for lower-capability coder agents working on the Netherica UI rework.

Use it together with:

- `ui_rework_task.md`
- exactly one detailed step file from `gui_rework_task/`

The recommended operating mode is one step per agent run.

---

## Purpose

The step files already contain the implementation details. This document adds:

- what context the coder must read first
- what they must not change
- how to validate their work
- what they must report back when done
- a ready-to-paste prompt template

---

## Handoff Strategy

Use one coder agent run per step.

Do not hand the whole UI rework to a weaker coder agent at once.

Recommended sequence:

1. Step 1
2. Step 2
3. Step 3
4. Step 4
5. Step 5
6. Step 6
7. Step 7
8. Step 8
9. Step 9

If a step depends on unfinished earlier work, finish the earlier step first.

---

## Required Inputs Per Run

Before coding, the agent must read:

1. `ui_rework_task.md`
2. the assigned step file, for example `gui_rework_task/ui_rework_task_step_4.md`
3. the current source files named in that step file’s `Current Anchors` section

For visual work, the agent must also read the relevant local design references under `gui_design/`.

---

## Non-Negotiable Guardrails

These rules apply to every UI rework step.

### Product / behavior guardrails

- Do not reintroduce Stitch references, Stitch IDs, or Stitch workflow.
- Do not reintroduce the stale `Inventory` navigation item.
- Do not change the crate version in `Cargo.toml` unless explicitly instructed.
- Do not add CRUD flows to Settings in this phase.
- Do not remove the transaction-date fallback acknowledgement gate before confirm.
- Do not remove report auto-open, report folder actions, report regeneration, or archive retry.
- Do not remove or weaken existing UI tests.

### Data / privacy guardrails

- There is a local `.xlsx` sample under `asset/` for local testing.
- It contains personal data.
- Never commit `.xlsx` samples.
- Never move them into embedded report assets.
- Do not print workbook contents or personal rows in final summaries.
- If you mention a test workbook in a summary, keep it generic.

### Font / asset guardrails

Current font layout is:

- `asset/fonts/Inter/`
- `asset/fonts/Sarabun/`
- `asset/fonts/NotoSansThaiLooped/`

Current runtime font usage assumptions:

- UI primary font: Inter
- UI Thai fallback: Noto Sans Thai Looped
- report embedded Thai-capable font: Sarabun

Do not move fonts again unless explicitly instructed.

---

## Current Repo Facts The Agent Should Assume

- Main runtime UI is currently in `src/ui/mod.rs`, though later steps may split it.
- Report assets are embedded from `asset/`.
- `.xlsx` files under `asset/` are gitignored and must remain excluded from embedded report assets.
- The main UI redesign source is local design reference under `gui_design/`, not Stitch.
- The shell target is `Ingestion`, `Reports`, and `Settings` only.
- Reports does not have a dedicated local mock; it should be adapted from current runtime capabilities.
- Settings is read-only in this phase.

---

## Validation Rules

Minimum validation for any code-bearing step:

1. `cargo check`
2. `cargo test`

If the step changes formatting-sensitive Rust code, also run:

3. `cargo fmt --check`

If the step is substantial enough to touch multiple modules or UI architecture, also run:

4. `cargo clippy --all-targets --all-features -- -D warnings`

For the final validation step, follow `gui_rework_task/ui_rework_task_step_9.md` exactly.

If validation fails because of your change, fix it before reporting completion.

If validation fails because of a clearly unrelated pre-existing issue, stop and report that blocker explicitly.

---

## Expected Delivery From The Coder Agent

When the coder finishes a step, their response should contain only these items:

1. `Completed` or `Blocked`
2. files changed
3. short summary of what was implemented
4. validation commands run and results
5. any unresolved blocker or follow-up risk

Preferred format:

```md
Status: Completed

Files changed:
- src/ui/mod.rs
- src/ui/worker.rs

What I implemented:
- Added structured worker progress messages
- Mapped parser events to UI worker events

Validation:
- cargo check ✅
- cargo test ✅

Risks / blockers:
- None
```

---

## Step-To-File Heuristic

Use this as a fast routing hint.

### Step 1

Likely files:

- `src/ui/mod.rs`
- `src/ui/theme.rs`
- `src/ui/sidebar.rs`
- `src/ui/components.rs`
- `src/ui/worker.rs`
- `src/ui/views/*`

### Step 2

Likely files:

- `src/main.rs`
- `src/ui/mod.rs`
- `src/ui/sidebar.rs`

### Step 3

Likely files:

- `src/ui/mod.rs`
- maybe `src/ui/views/dry_run.rs`
- maybe `src/ui/views/complete.rs`

### Step 4

Likely files:

- `src/ui/worker.rs`
- `src/ui/mod.rs`
- `src/ingestion.rs`

### Step 5

Likely files:

- `src/ui/theme.rs`
- `src/ui/components.rs`
- maybe overlay rendering locations in `src/ui/mod.rs` or extracted view/component files

### Step 6

Likely files:

- `src/ui/views/idle.rs`
- `src/ui/views/progress.rs`
- `src/ui/views/dry_run.rs`
- `src/ui/views/complete.rs`

### Step 7

Likely files:

- `src/ui/views/settings.rs`
- `src/ui/views/reports.rs`
- `src/ui/mod.rs`

### Step 8

Likely files:

- all `src/ui/*` modules touched by cleanup

### Step 9

Likely files:

- no code changes if everything already passes
- possibly small polish fixes discovered during validation

---

## Ready-To-Paste Prompt Template

Use this template when handing off a single step.

```md
Implement only this Netherica UI rework step:

STEP FILE: `gui_rework_task/ui_rework_task_step_X.md`

Read first:
1. `ui_rework_task.md`
2. `gui_rework_task/ui_rework_task_step_X.md`
3. every source file named in that step file's `Current Anchors`

Requirements:
- complete only this step
- keep changes minimal and direct
- preserve all existing behavior unless the step explicitly changes it
- do not change crate version
- do not add Settings CRUD
- do not reintroduce `Inventory`
- do not remove fallback acknowledgement before confirm
- do not remove report open / open folder / regenerate / retry archive flows
- do not commit or expose personal `.xlsx` sample data under `asset/`
- keep font paths aligned with current asset layout:
  - `asset/fonts/Inter/`
  - `asset/fonts/Sarabun/`
  - `asset/fonts/NotoSansThaiLooped/`

Validation required before you finish:
- `cargo check`
- `cargo test`
- `cargo fmt --check`
- run `cargo clippy --all-targets --all-features -- -D warnings` if the step is substantial or touches multiple modules

When done, return:
1. status: Completed or Blocked
2. files changed
3. concise implementation summary
4. validation results
5. blockers or risks
```

---

## Operator Notes

If the coder is weak, do not ask them to choose architecture. Pick the step file yourself and give them one step only.

Good handoff example:

- assign Step 4 only
- include the exact step file path
- tell them not to touch unrelated steps
- require validation output

Bad handoff example:

- “please do the whole UI rework”
- “use your judgment on architecture”
- “improve whatever seems necessary”

---

## Recommended First Handoffs

If you want to de-risk the project, use this order:

1. Step 1 - module split without behavior change
2. Step 2 - navigation and shell
3. Step 4 - worker message structure
4. Step 6 - ingestion workflow screens

This sequence tends to reduce merge pain and keeps visual work from being blocked on architecture.
