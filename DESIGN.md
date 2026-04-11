# DESIGN.md — Netherica Design System

> **Source of Truth:** Stitch MCP project `netherica design` (ID `18330809547273064391`)
> **Design System:** Nordic Precision (asset `e2cc8575c8464ff5b92d6dc0bd036584`)
> **Creative North Star:** _"The Arctic Atelier"_
> **Mode:** Dark · **Font:** Inter · **Roundness:** Round-4 · **Spacing Scale:** 2
> **Last synced:** 2026-04-11

---

## Table of Contents

1. [Philosophy & North Star](#1-philosophy--north-star)
2. [Color Tokens](#2-color-tokens)
3. [Typography](#3-typography)
4. [Spacing & Layout](#4-spacing--layout)
5. [Elevation & Depth](#5-elevation--depth)
6. [Component Guidelines](#6-component-guidelines)
7. [Do's and Don'ts](#7-dos-and-donts)
8. [Quick-Reference Token Table](#8-quick-reference-token-table)

---

## 1. Philosophy & North Star

The Netherica UI is not a traditional enterprise dashboard. It is a **high-end editorial space**—a sanctuary for focus.

| Principle | Description |
|---|---|
| **Atmospheric Depth** | Prioritize tonal layering over structural rigidity. The UI should feel like frosted glass sheets on a dark, infinite landscape. |
| **No-Line Rule** | Traditional `1px solid` borders are **prohibited** for sectioning. Boundaries are defined solely through background-color shifts. |
| **Tonal Layering** | Elevation is communicated by shifting surface tokens, not by adding drop shadows. |
| **Breathing Room** | When a section feels crowded, increase spacing—never add a divider line. |

---

## 2. Color Tokens

### 2.1 Brand Colors

| Token | Hex | Role |
|---|---|---|
| `primary` | `#a3dcec` | Primary interactive elements, links, active indicators |
| `primary-container` | `#88c0d0` | Gradient endpoint, contained primary surfaces |
| `secondary` | `#a9caeb` | Secondary interactive elements |
| `secondary-container` | `#2b4c68` | Contained secondary surfaces |
| `tertiary` | `#f0cf8f` | Warnings, accents, Aurora palette |
| `tertiary-container` | `#d2b476` | Contained tertiary surfaces |

### 2.2 Surface Hierarchy (The Tonal Landscape)

These tokens define layered depth from darkest (base) to brightest. **Use these instead of borders.**

| Token | Hex | Usage |
|---|---|---|
| `surface-container-lowest` | `#080e19` | Input field fills, deepest recesses |
| `surface` / `surface-dim` | `#0d131e` | **Base Layer** — The infinite background canvas |
| `surface-container-low` | `#161c27` | **Secondary Layer** — Sidebars, secondary panels |
| `surface-container` | `#1a202b` | **Content Layer** — Primary workspace, card backgrounds |
| `surface-container-high` | `#242a36` | **Interactive Layer** — Active states, hover targets, floating elements |
| `surface-container-highest` | `#2f3541` | Elevated overlays, inactive wizard segments |
| `surface-variant` | `#2f3541` | Glassmorphism base (use at partial opacity) |
| `surface-bright` | `#333945` | Maximum surface brightness |

### 2.3 Text / "On" Colors

| Token | Hex | Usage |
|---|---|---|
| `on-surface` | `#dde2f2` | **Primary text** — critical information |
| `on-surface-variant` | `#c0c8cb` | **Secondary text** — descriptions, metadata |
| `on-background` | `#dde2f2` | Text on base background |
| `on-primary` | `#003640` | Text on primary surfaces |
| `on-primary-container` | `#0c4f5d` | Text on primary containers |
| `on-secondary` | `#0e334e` | Text on secondary surfaces |
| `on-secondary-container` | `#9bbcdd` | Text on secondary containers |
| `on-tertiary` | `#402d00` | Text on tertiary surfaces |
| `on-tertiary-container` | `#5b4512` | Text on tertiary containers |

### 2.4 Error / Status Colors (Aurora Palette)

| Token | Hex | Usage |
|---|---|---|
| `error` | `#ffb4ab` | Failed states — as 4px status dots or soft glows only |
| `error-container` | `#93000a` | Error surface |
| `on-error` | `#690005` | Text on error |
| `on-error-container` | `#ffdad6` | Text on error containers |
| `tertiary` | `#f0cf8f` | Warnings — as 4px status dots only |

> **Rule:** Error and warning colors must _never_ be used as heavy background fills. Only as small status indicators.

### 2.5 Outline / Border Colors

| Token | Hex | Usage |
|---|---|---|
| `outline` | `#8a9295` | Standard outlines (use sparingly) |
| `outline-variant` | `#40484b` | **Ghost Border** — used at 15% opacity for data table boundaries |

### 2.6 Inverse & Fixed Tokens

| Token | Hex |
|---|---|
| `inverse-surface` | `#dde2f2` |
| `inverse-on-surface` | `#2b313c` |
| `inverse-primary` | `#2b6674` |
| `surface-tint` | `#97cfe0` |
| `primary-fixed` | `#b3ecfc` |
| `primary-fixed-dim` | `#97cfe0` |
| `secondary-fixed` | `#cee5ff` |
| `secondary-fixed-dim` | `#a9caeb` |
| `tertiary-fixed` | `#ffdf9f` |
| `tertiary-fixed-dim` | `#e2c383` |

### 2.7 Override Palette (Brand Anchors)

These are the four anchor colors from which the entire Material token set was generated:

| Role | Hex |
|---|---|
| **Primary** | `#88c0d0` |
| **Secondary** | `#81a1c1` |
| **Tertiary** | `#ebcb8b` |
| **Neutral** | `#2e3440` |

### 2.8 The "Glass & Gradient" Rule

For primary CTAs and action surfaces, apply a **135° linear gradient**:

```
from: primary   (#a3dcec)
  to: primary-container (#88c0d0)
angle: 135deg
```

This prevents the flat look and provides a tactile, premium feel. **Example (CSS):**

```css
.btn-primary {
  background: linear-gradient(135deg, #a3dcec, #88c0d0);
  color: #003640; /* on-primary */
}
```

---

## 3. Typography

### 3.1 Font Stack

| Slot | Font | Rationale |
|---|---|---|
| **Headlines** | Inter | Mathematical precision, editorial authority |
| **Body** | Inter | Consistent reading rhythm |
| **Labels** | Inter | Small-size legibility |
| **Thai Fallback** | Sarabun | Seamless Thai script integration |

### 3.2 Type Scale

| Token | Size | Weight | Tracking | Usage |
|---|---|---|---|---|
| `headline-lg` | 2rem (32px) | Bold | −0.02em | Page titles, display numbers |
| `headline-md` | 1.5rem (24px) | Semi-Bold | −0.01em | Section headings |
| `headline-sm` | 1.25rem (20px) | Semi-Bold | normal | Sub-section headings |
| `body-md` | 0.875rem (14px) | Regular | normal | Primary body text |
| `body-sm` | 0.8125rem (13px) | Regular | normal | Secondary body text |
| `label-md` | 0.75rem (12px) | Medium | **+0.05em, ALL CAPS** | Wizard step titles, table metadata |
| `label-sm` | 0.6875rem (11px) | Medium | **+0.05em, UPPERCASE** | Table headers, micro-labels |

### 3.3 Color Pairing

| Priority | Token | Hex | When |
|---|---|---|---|
| **High** | `on-surface` | `#dde2f2` | Critical info, primary headings |
| **Medium** | `on-surface-variant` | `#c0c8cb` | Descriptions, secondary text, metadata |
| **Accent** | `primary` | `#a3dcec` | Focused labels, interactive text |

> **Rule:** Always pair `headline-sm` (Bold) with `body-sm` (Regular, `on-surface-variant`) for a high-contrast professional feel.

---

## 4. Spacing & Layout

### 4.1 Base Unit

The Stitch project defines a **spacing scale of `2`**, meaning the base unit is multiplied by 2. The recommended base is **4px**, giving us:

| Token | Value | Usage |
|---|---|---|
| `space-1` | 4px | Micro-gaps (icon-to-label) |
| `space-2` | 8px | Small internal padding |
| `space-3` | 12px | Default element spacing |
| `space-4` | 16px | Standard card padding |
| `space-5` | 20px | Comfortable section gaps |
| `space-6` | 24px | Standard section margins |
| `space-8` | 32px | Large section dividers |
| `space-10` | 40px | Page-level margins |
| `space-12` | 48px | Major layout separators |
| `space-16` | 64px | Maximum breathing room |

### 4.2 Corner Radius

The design system uses **Round-4** (`ROUND_FOUR`):

| Token | Value | Usage |
|---|---|---|
| `radius-sm` | 0.25rem (4px) | Default small elements |
| `radius-md` | 0.375rem (6px) | Buttons, inputs |
| `radius-lg` | 0.5rem (8px) | Medium containers |
| `radius-xl` | 0.75rem (12px) | Large cards, settings panels |

### 4.3 Layout Principles

1. **Asymmetrical Margins:** Use more padding on the left than the right in headers to create a bespoke, non-bootstrap feel.
2. **Data Table Row Height:** `1.5rem` minimum vertical space between rows — no grid lines.
3. **Breathing Room:** If a section feels crowded, increase spacing—never add a divider.

---

## 5. Elevation & Depth

### 5.1 The Layering Principle (Primary Method)

**Depth is achieved through Tonal Layering, not drop shadows.**

To "lift" a card: change its background from `surface-container-low` → `surface-container`. The 2% brightness shift is sufficient for the human eye to perceive elevation in a dark environment.

```
Deepest   ← surface-container-lowest (#080e19)
             surface                  (#0d131e)
             surface-container-low    (#161c27)
             surface-container        (#1a202b)
             surface-container-high   (#242a36)
Highest   → surface-container-highest (#2f3541)
```

### 5.2 Ambient Shadows (Floating Modals Only)

For floating modals and overlays that must feel detached from the surface:

```css
.modal-floating {
  box-shadow: 0 24px 48px rgba(0, 0, 0, 0.4);
}
```

> The shadow must be tinted with a hint of the background color to feel natural. Never use generic black shadows.

### 5.3 The "Ghost Border" Fallback

For when a container _requires_ a visible boundary (e.g., high-density data tables):

```css
.ghost-border {
  border: 1px solid rgba(64, 72, 75, 0.15); /* outline-variant at 15% */
}

/* Hover state */
.ghost-border:hover {
  border-color: rgba(64, 72, 75, 0.30); /* 30% on interaction */
}
```

### 5.4 Glassmorphism (Wizards & Overlays)

For navigation overlays and the progress wizard:

```css
.glass-overlay {
  background: rgba(47, 53, 65, 0.7); /* surface-container-highest at 70% */
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
}
```

---

## 6. Component Guidelines

### 6.1 Buttons

| Variant | Background | Text | Radius |
|---|---|---|---|
| **Primary** | Gradient `#a3dcec → #88c0d0` (135°) | `on-primary` (`#003640`) | `radius-md` (0.375rem) |
| **Secondary** | `surface-container-highest` | `on-surface` | `radius-md` |
| **Ghost/Tertiary** | Transparent | `primary` | `radius-md` |

### 6.2 Input Fields

| Property | Value |
|---|---|
| **Fill** | `surface-container-lowest` (`#080e19`) |
| **Border** | None (default state) |
| **Focus border** | Ghost Border at 40% opacity (`rgba(64, 72, 75, 0.4)`) |
| **Focus label color** | `primary` (`#a3dcec`) |
| **Label style** | `label-sm`, uppercase |
| **Radius** | `radius-md` |

### 6.3 Data Tables

| Property | Rule |
|---|---|
| **Internal grid lines** | **Forbidden** |
| **Row separation** | Vertical whitespace (`1.5rem` row height) + subtle hover |
| **Hover row** | Transition to `surface-container-high` |
| **Header background** | `surface-container-low` |
| **Header typography** | `label-sm`, uppercase, `on-surface-variant` |
| **Status indicators** | 4px dots only: `error` for failed, `tertiary` for warnings |

### 6.4 Cards (Settings Grid)

| Property | Value |
|---|---|
| **Radius** | `radius-xl` (0.75rem) |
| **Background** | `surface-container-low` on `surface` parent |
| **Hover** | Background → `surface-container-highest`, Ghost Border → 30% |
| **Transition** | Do NOT move/translate the card. Background shift only. |

### 6.5 Progress Wizard

| Property | Rule |
|---|---|
| **Form factor** | Horizontal bar, **not** circles-and-lines |
| **Inactive segments** | `surface-container-highest` |
| **Active segment** | Gradient: `primary` → `secondary` |
| **Step labels** | `label-md`, ALL CAPS, placed _above_ the bar, left-aligned |

---

## 7. Do's and Don'ts

### ✅ Do

| Rule | Why |
|---|---|
| Use asymmetrical margins (e.g., more left padding in headers) | Creates a bespoke, non-template feel |
| Use Snow Storm colors (`#d8dee9`–`#dde2f2`) for primary text | Ensures high-contrast readability on dark backgrounds |
| Allow generous "Breathing Room" | Increase spacing instead of adding dividers |
| Use `surface` tokens exclusively for backgrounds | Maintains the Nord "frosty" atmosphere |
| Use status indicators as 4px dots or soft glows | Keeps the muted, professional aesthetic |

### ❌ Don't

| Rule | Why |
|---|---|
| Use pure black (`#000000`) | Breaks the Nord tonal atmosphere |
| Use standard "heavy" drop shadows | Muddies the palette's clarity |
| Use high-saturation reds or greens | Clashes with the Aurora/Frost tokens |
| Use `1px solid` borders for sectioning | Violates the No-Line Rule; use tonal layering |
| Use heavy background fills for error/warning states | Use small status dots instead |

---

## 8. Quick-Reference Token Table

### Surfaces (Dark → Light)

```
#080e19  surface-container-lowest   Deepest recess / input fills
#0d131e  surface / background       Base canvas
#161c27  surface-container-low      Sidebars, panels
#1a202b  surface-container          Workspace, cards
#242a36  surface-container-high     Hover, active states
#2f3541  surface-container-highest  Overlays, inactive wizards
#333945  surface-bright             Maximum brightness
```

### Primary Palette

```
#a3dcec  primary                Active elements
#88c0d0  primary-container      Gradient endpoint
#b3ecfc  primary-fixed          Fixed primary (accessibility)
#97cfe0  primary-fixed-dim      Dimmed fixed primary
#003640  on-primary             Text on primary
#0c4f5d  on-primary-container   Text on primary container
```

### Secondary Palette

```
#a9caeb  secondary              Secondary elements
#2b4c68  secondary-container    Secondary container
#cee5ff  secondary-fixed        Fixed secondary
#0e334e  on-secondary           Text on secondary
#9bbcdd  on-secondary-container Text on secondary container
```

### Tertiary / Aurora Palette

```
#f0cf8f  tertiary               Warning accents
#d2b476  tertiary-container     Tertiary container
#ffdf9f  tertiary-fixed         Fixed tertiary
#402d00  on-tertiary            Text on tertiary
#5b4512  on-tertiary-container  Text on tertiary container
```

### Error Palette

```
#ffb4ab  error                  Error indicators (dots only)
#93000a  error-container        Error surface
#690005  on-error               Text on error
#ffdad6  on-error-container     Text on error container
```

---

> **This document is the single source of truth for all Netherica UI decisions.**
> All implementations (Rust TUI, web preview, print/export) must reference these tokens.
> Any deviation requires an explicit Architecture Decision Record (ADR).
