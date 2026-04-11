$filePath = 'd:\Projects\netherica\Netherica_rqrmnt.md'
$content = [System.IO.File]::ReadAllText($filePath)

# Define the replacement text for Section 7
$newSection7 = @'
## 7. Graphical User Interface (GUI)

> **Design Source:** Stitch MCP project `netherica design` (ID `18330809547273064391`)
> **Design System:** Nordic Precision — Creative North Star: _"The Arctic Atelier"_
> **Full Specification:** See `DESIGN.md` for complete design tokens, color palette, typography scale, and component guidelines.

### 7.1 Design System Summary

The Netherica GUI follows the **"Editorial Calm"** design philosophy: a high-end editorial space prioritizing atmospheric depth over structural rigidity. The interface uses **egui/eframe** with custom theming mapped to the Nordic Precision token set.

#### Core Design Principles

| Principle | Implementation |
|---|---|
| **Dark Mode** | Base canvas `#0d131e`, never pure black `#000000` |
| **No-Line Rule** | 1px borders prohibited for sectioning; use background-color shifts only |
| **Tonal Layering** | Elevation via surface token progression, not drop shadows |
| **Glass & Gradient** | Primary CTAs use 135-degree gradient (`#a3dcec` to `#88c0d0`) |
| **Breathing Room** | Increase spacing when crowded -- never add divider lines |

#### Surface Hierarchy (egui `Visuals` mapping)

| Layer | Token | Hex | Usage |
|---|---|---|---|
| Deepest | `surface-container-lowest` | `#080e19` | Input field fills |
| Base | `surface` | `#0d131e` | Window background canvas |
| Secondary | `surface-container-low` | `#161c27` | Sidebar, secondary panels |
| Content | `surface-container` | `#1a202b` | Primary workspace, cards |
| Interactive | `surface-container-high` | `#242a36` | Hover states, active elements |
| Overlay | `surface-container-highest` | `#2f3541` | Modals, inactive wizard |

#### Typography

| Slot | Font | Size | Notes |
|---|---|---|---|
| Headlines | Inter | 2rem (32px), -0.02em tracking | Editorial authority |
| Body | Inter | 0.875rem (14px) | Primary content |
| Labels | Inter | 0.75rem (12px), ALL CAPS, +0.05em | Table headers, wizard steps |
| Thai Fallback | Sarabun (THSarabunNew.ttf) | Matches body sizing | Embedded via `rust-embed` |

#### Core Color Tokens

| Token | Hex | Role |
|---|---|---|
| `primary` | `#a3dcec` | Active elements, links, focused labels |
| `primary-container` | `#88c0d0` | Gradient endpoint, contained primary |
| `secondary` | `#a9caeb` | Secondary interactive elements |
| `tertiary` | `#f0cf8f` | Warning indicators (4px dots only) |
| `error` | `#ffb4ab` | Error indicators (4px dots only, never background fills) |
| `on-surface` | `#dde2f2` | Primary text |
| `on-surface-variant` | `#c0c8cb` | Secondary text, descriptions |
| `outline-variant` | `#40484b` | Ghost borders at 15% opacity |

### 7.2 Application Layout

The application uses a **persistent sidebar + central content area** layout:

```
+--------------------------------------------------+
|  +----------+  +-------------------------------+ |
|  |          |  |                               | |
|  |  Sidebar |  |       Central Content         | |
|  |          |  |                               | |
|  |  - Logo  |  |   (View-specific content      | |
|  |  - Nav   |  |    determined by current       | |
|  |    items |  |    navigation + workflow       | |
|  |          |  |    state)                      | |
|  |          |  |                               | |
|  |          |  |                               | |
|  |          |  |                               | |
|  +----------+  +-------------------------------+ |
|  Footer: "Precision Reconciliation Engine"       |
+--------------------------------------------------+
```

#### Sidebar (Persistent)

Background: `surface-container-low` (`#161c27`)

| Icon | Label | Section | Description |
|---|---|---|---|
| `upload_file` | **Ingestion** | Primary workflow | File upload, parsing, review, commit |
| `analytics` | **Reports** | Report browser | Generated report access (future scope) |
| `inventory_2` | **Inventory** | Inventory viewer | Current product totals (future scope) |
| `settings` | **Settings** | Configuration | Products, Departments, App settings |

- **Active item:** `primary` (`#a3dcec`) text color + `surface-container` background highlight
- **Inactive items:** `on-surface-variant` (`#c0c8cb`) text
- Logo: "Netherica" headline + "Pharmacy Reconciliation" subtitle

### 7.3 Ingestion View States

The Ingestion section uses a linear workflow state machine:

```
+-------+     +---------+     +--------+     +------------+     +----------+
| Idle  |---->| Parsing |---->| Review |---->| Committing |---->| Complete |
+-------+     +---------+     |(DryRun)|     +------------+     +----------+
   ^                          +---+----+                            |
   |                              | Cancel                          |
   |                              v                                 |
   |                           +------+                             |
   +---------------------------| Idle |<----------------------------+
                               +------+         "New File"
```

#### 7.3.1 Idle View

- **Headline:** "Ready for new reconciliation run?"
- **Description:** "Upload your latest Pharmacy Excel export to begin the automated discrepancy audit."
- **Primary Action:** File upload drop zone / button -- triggers `rfd` native file picker
- **Status Cards** (3-column grid, `surface-container-low` cards):
  - **Last Run:** Timestamp of most recent `file_history` entry (or "No files processed" if empty)
  - **Sync State:** Database connection status: Connected / Fallback path used
  - **Config Status:** Product/department count summary from `config.toml`

#### 7.3.2 Parsing View

- **Headline:** "Analyzing Data Structure"
- **Sub-headline:** "Parsing sheet: {current_sheet_name}..."
- **File Metadata Card** (`surface-container` background):
  - Filename, file size, detected sheet count
  - List of matched sheet names
- **Live Log Output** (terminal-style, `surface-container-lowest` background):
  - Scrolling log prefixed with timestamps: `[HH:MM:SS] LEVEL: message`
  - Examples: `[14:22:01] INFO: Initializing stream reader for 'GAUZE-01'...`
  - Warnings highlighted with `tertiary` (`#f0cf8f`) text
- **Progress Bar:**
  - Track: `surface-container-highest`
  - Fill: Gradient `primary` to `secondary` (`#a3dcec` to `#a9caeb`)
  - Label: Current row / total rows per sheet
- **No user-interruptible actions** during parsing (background thread via `mpsc`)

#### 7.3.3 Review View (Dry Run)

- **Headline:** "Dry Run Review"
- **Description:** "Validate reconciliation metrics before generating the final ledger report."
- **Summary Metric Cards** (3-column, `surface-container-low`):
  - **Accuracy Score:** Variance threshold indicator
  - **Inventory Warnings:** Count of products with potential stock-out or unusual deltas
  - **Computation Time:** Elapsed time for dry run calculation
- **Reconciliation Ledger Table** (`egui_extras::TableBuilder`):

| Product | Department Breakdown | Opening Leftover (subunits) | Total Subunits Used | Whole Units Output | Closing Leftover (subunits) |
|---------|----------------------|-----------------------------|---------------------|--------------------|----------------------------|

  - **Table styling:**
    - Header: `surface-container-low` background, `label-sm` uppercase text
    - Rows: No grid lines, `1.5rem` row height, hover to `surface-container-high`
    - Ghost borders at 15% opacity for column separation if needed
  - **Department Breakdown:** Concatenated string, e.g., `ER: 150, OPD: -20`
  - **Opening Leftover:** Computed from ledger before this file's transaction date
  - **Total Subunits Used:** Sum of `dispensed_amount` from this file (per product)
  - **Whole Units Output:** As defined in Section 6.2
  - **Closing Leftover:** Opening + total subunits used, modulo factor
  - **Row count indicator:** "Showing {visible} of {total} reconciled products"
- **Status Indicators:** 4px dots -- `error` for failures, `tertiary` for warnings
- **Confirmation Footer:**
  - Warning text: "Confirming will finalize the period-end reconciliation and update the central inventory ledger."
  - Buttons: `[Cancel]` (secondary, back to Idle) and `[Confirm & Generate Report]` (primary, gradient)

#### 7.3.4 Completion View

- **Headline:** "Process Completion"
- **Success Banner:** "Reconciliation Successful" with checkmark icon
  - Description: "Report successfully generated for {filename}. The pharmaceutical assets have been synchronized with the master ledger."
- **Summary Metrics** (inline):
  - **Rows Processed:** Total ledger entries created
  - **Discrepancies Resolved:** Count (usually 0 for clean runs)
- **Action Cards** (2-column, `surface-container-low`):
  - **"Open Report Folder"** -- Opens `./reports/` directory in system file manager
  - **"Regenerate Last Report"** -- Re-queries ledger and recreates HTML
- **New Cycle CTA:** "Begin a new ingestion cycle for the next inventory manifest or clinical batch file." -- Button returns to Idle
- **System Health & Metadata** (footer section, `label-md` uppercase):
  - Execution Time | Data Integrity % | Validator Version | Log Hash
- **Footer:** "Precision Reconciliation Engine"

### 7.4 Settings Views

The Settings section contains two sub-views accessible via tabs or sub-navigation.

#### 7.4.1 Settings: Departments

- **Headline:** "Department Configuration"
- **Section Title:** "Department Registry"
- **Description:** "Manage departmental taxonomy and display mapping."
- **Department List** (card-based grid, `surface-container-low` cards with `radius-xl`):
  - Each card displays:
    - **Department Code** (monospace, `label-md` uppercase)
    - **Mapped Display Name** (headline-sm)
  - Hover: Background shifts to `surface-container-highest`, ghost border at 30%
- **Add Department:**
  - Section: "Provision New Node" / "Register Department"
  - Input fields: Department Code + Mapped Display Name
  - Uses `surface-container-lowest` fills, no borders, focus ghost border at 40%
  - Validation: Code must be unique, non-empty
- **Relationship to `config.toml`:** Edits here update the `[departments]` section

#### 7.4.2 Settings: Products

- **Headline:** "Product Configuration"
- **Section Title:** "Inventory Ledger"
- **Description:** "Configure medical supplies, reconciliation factors, and tracking units."
- **Product List** (card-based grid, `surface-container-low` cards):
  - Each card displays:
    - **Product ID** (monospace, `label-md`)
    - **Display Name** (headline-sm)
    - Details: Unit, Subunit, Factor, Track Subunits toggle
  - Hover: Background shifts to `surface-container-highest`, ghost border at 30%
- **Add New Entry:**
  - Section: "Create a new product definition for the reconciliation engine."
  - Fields: ID, Display Name, Unit, Subunit, Factor, Track Subunits
  - Validation: ID must be unique, factor > 0, if `track_subunits = false` then `factor = "1"`
- **System Status Footer:**
  - Configuration Integrity percentage
  - Sync status with config file
- **Relationship to `config.toml`:** Edits here update the `[[products]]` entries

### 7.5 Error Handling & Notifications

| Severity | Display Method | Token | Examples |
|---|---|---|---|
| **Critical** | Modal dialog (glassmorphism overlay) | `error` / `error-container` | File lock, duplicate hash, chronological violation |
| **Warning** | Toast notification (auto-dismiss) | `tertiary` | Missing column on one sheet, null date fallback |
| **Info** | Inline status or log entry | `on-surface-variant` | Sheet skipped, row parsed |

- **Toast Notifications:**
  - Position: Top-right overlay
  - Background: `surface-container-highest` with glassmorphism (`backdrop-filter: blur(12px)`)
  - Auto-dismiss after 5 seconds, manually dismissible
- **Modal Dialogs (Critical):**
  - Background overlay: `surface-variant` at 70% opacity with blur
  - Modal card: `surface-container-high` with ambient shadow (`0 24px 48px rgba(0,0,0,0.4)`)
  - Action buttons: Acknowledge / Retry as appropriate
- **All warnings/errors** written to `pharmacy_app.log` (structured, rotated daily, max 5 MB)

### 7.6 Component Quick Reference (egui Mapping)

| Stitch Component | egui Implementation | Key Token |
|---|---|---|
| Primary Button | `egui::Button` with custom `Visuals` | Gradient `#a3dcec` to `#88c0d0` |
| Secondary Button | `egui::Button` with `surface-container-highest` fill | `#2f3541` |
| Ghost Button | `egui::Button` with transparent fill | `primary` text only |
| Input Field | `egui::TextEdit` | `surface-container-lowest` fill |
| Data Table | `egui_extras::TableBuilder` | No grid lines, hover rows |
| Settings Card | `egui::Frame` with `radius-xl` | `surface-container-low` |
| Progress Bar | `egui::ProgressBar` custom painted | Gradient fill |
| Toast | Custom overlay widget | Glassmorphism |
| Modal | Custom overlay with backdrop | Ambient shadow |
'@

# Find Section 7 and Section 8 boundaries
$startPattern = "## 7. Graphical User Interface (GUI)"
$endPattern = "## 8. Reporting Engine"

$startIdx = $content.IndexOf($startPattern)
$endIdx = $content.IndexOf($endPattern)

if ($startIdx -eq -1) {
    Write-Host "ERROR: Could not find Section 7 start marker"
    exit 1
}
if ($endIdx -eq -1) {
    Write-Host "ERROR: Could not find Section 8 start marker"
    exit 1
}

Write-Host "Found Section 7 at index: $startIdx"
Write-Host "Found Section 8 at index: $endIdx"

# Find the separator (---) before Section 8
# We want to keep "---" before Section 8 as a separator after our new Section 7
$beforeSection8 = $content.Substring(0, $endIdx)
$lastSeparatorIdx = $beforeSection8.LastIndexOf("---")

if ($lastSeparatorIdx -gt $startIdx) {
    # Replace from Section 7 start to the --- separator (inclusive of the ---)
    $before = $content.Substring(0, $startIdx)
    $after = $content.Substring($lastSeparatorIdx) # keeps the "---" and everything after
    $newContent = $before + $newSection7 + "`r`n`r`n" + $after
} else {
    # Fallback: just replace up to Section 8
    $before = $content.Substring(0, $startIdx)
    $after = $content.Substring($endIdx)
    $newContent = $before + $newSection7 + "`r`n`r`n---`r`n`r`n" + $after
}

[System.IO.File]::WriteAllText($filePath, $newContent, [System.Text.Encoding]::UTF8)
Write-Host "SUCCESS: Section 7 replaced successfully"
