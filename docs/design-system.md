# Design System

The design system is the rulebook for visual decisions in the GUI. It exists so that adding a new tab, widget, or info card looks like it belongs to the same app — not like a separate experiment glued on the side.

The system is **dark-only**. Token names are theme-agnostic (`SURFACE`, not `DARK_BG`) so a future light theme is a recolor, not a rewrite.

The source of truth lives in two files:

- [`src/gui/colors.rs`](../src/gui/colors.rs) — the palette
- [`src/gui/tokens.rs`](../src/gui/tokens.rs) — spacing, shape, typography scale, control sizing, shadows, icon sizing

Components built on top live in [`src/gui/widgets.rs`](../src/gui/widgets.rs). Always reach for an existing widget before inventing a new pattern.

---

## 1. Foundations

### 1.1 Color

#### Surfaces

A three-tier surface scale frames the chrome. Lower index = base layer; higher = floating above.

| Token | Value | Where |
|---|---|---|
| `SURFACE` | `gray(22)` | Central panel, base background |
| `SURFACE_ALT` | `gray(26)` | Topbar, statusbar, list rows |
| `SURFACE_RAISED` (= `CARD_FILL`) | `gray(30)` | Cards, popovers, raised tiles |

#### Borders & strokes

| Token | Value | Where |
|---|---|---|
| `BORDER` | `gray(50)` | Hairline borders (default) |
| `BORDER_STRONG` | `gray(70)` | Hover/emphasis borders |
| `STROKE_HAIRLINE` | `0.5px` | Default border width |
| `STROKE_ACCENT` | `1.0px` | Focus / selection emphasis |

#### Interaction states

| Token | Value | Where |
|---|---|---|
| `HOVER_FILL` | `gray(42)` | Widget hover background |
| `ACTIVE_FILL` | `gray(54)` | Pressed / open background |
| `FOCUS_RING` | `rgb(100,180,190)` | Focus stroke, hyperlinks, selection edge |
| `SELECTION_BG` | `rgb(48,100,112)` | Text/row selection fill |

The accent is deliberately teal — it must not collide with the P2 slot blue.

#### Text labels

Greyscale ramp for label hierarchy.

| Token | Value | Where |
|---|---|---|
| `LABEL_DIM` | `gray(130)` | Secondary metadata |
| `LABEL_DIMMER` | `gray(100)` | Disabled / placeholder |
| `LABEL_SOFT` | `gray(170)` | Default body label |
| `LABEL_STRONG` | `gray(200)` | Emphasized label |

Pure white (`Color32::WHITE`) is reserved for player names inside chips and for high-contrast counters in cards. Don't use white for ordinary body text — `LABEL_SOFT` is the right default.

#### Semantic accents

| Token | Value | Where |
|---|---|---|
| `ACCENT_SUCCESS` | `rgb(140,200,110)` | Wins, positive deltas |
| `ACCENT_WARNING` | `rgb(230,170,60)` | Supply blocks, inactive states |
| `ACCENT_DANGER` | `rgb(220,90,90)` | Losses, destructive actions, errors |

#### Player & race colors

The app has **two** player-color systems with strict, non-overlapping uses:

| System | Tokens | Use for |
|---|---|---|
| **Slot** (canonical) | `player_slot_color(idx)`, `player_slot_color_bright(idx)` | Anything that asks "which player" — names, chart series, side panels, build-order columns, hero stripes |
| **Race** | `RACE_TERRAN`, `RACE_PROTOSS`, `RACE_ZERG`, `race_color(race)` | Anything that asks "which faction" — race badges (T/P/Z), matchup codes (`TvZ` colored letter-by-letter), race filter chips |

For 3+ players (FFA, observer replays), `salt::*` derives a deterministic color from the player name as fallback when slot 2+ would all collapse to neutral grey.

**Race colors are sacred.** Don't retune them — they double as legend colors in race filter chips, and changing them ripples across the library hero strip and matchup badges.

### 1.2 Typography

Four-step scale, every size resolves against the user's `font_size` slider (clamped 8.0..=28.0). Helpers live in `tokens.rs`.

| Function | Multiplier | Use for |
|---|---|---|
| `size_caption(cfg)` | × 0.72 | Hints, legends, secondary labels |
| `size_body(cfg)` | × 1.00 | Default text |
| `size_subtitle(cfg)` | × 1.15 | Card headers, player names in cards |
| `size_title(cfg)` | × 1.43 | Section headings |

When you write `RichText::new(...).size(...)`, always pass one of these — never a literal. That's how the user's font slider works.

`RichText` decorations are layered freely:
- `.strong()` for player names, section headings
- `.italics()` for hints and placeholder text
- `.monospace()` for numbers, paths, timestamps
- `.color(...)` from the palette above

### 1.3 Spacing

| Token | Value | Use for |
|---|---|---|
| `SPACE_XS` | `2.0` | Tight bar paddings, vertical dividers |
| `SPACE_S` | `4.0` | Default `item_spacing.y`, compact gaps |
| `SPACE_M` | `8.0` | Standard inter-section gap |
| `SPACE_L` | `12.0` | Section breaks |
| `SPACE_XL` | `16.0` | Major visual breaks |
| `SPACE_XXL` | `24.0` | Empty-state vertical padding |

Never `ui.add_space(N.0)` with a literal — pick a token.

### 1.4 Shape

| Token | Value | Use for |
|---|---|---|
| `RADIUS_BUTTON` | `4.0` | Buttons, row frames |
| `RADIUS_CARD` | `6.0` | Card surfaces |
| `RADIUS_WINDOW` | `8.0` | Modals |
| `RADIUS_CHIP` | `10.0` | Pill toggles, removable chips |

#### Inner margins (for `egui::Frame`)

| Token | Value | Use for |
|---|---|---|
| `CARD_INNER_MX` / `MY` | `12` / `10` | Card frames |
| `CHIP_INNER_MX` / `MY` | `8` / `3` | Chips, removable chips |
| `ROW_INNER_MX` / `MY` | `12` / `5` | Library list rows (denser than cards) |

### 1.5 Control sizing

Three steps for buttons, inputs, transport controls.

| Token | Value | Use for |
|---|---|---|
| `CONTROL_HEIGHT_S` (= `CHIP_MIN_HEIGHT`) | `22.0` | Small buttons, chips |
| `CONTROL_HEIGHT_M` | `28.0` | Search inputs, toolbar buttons |
| `CONTROL_HEIGHT_L` | `36.0` | Transport (play/pause/speed) |

### 1.6 Topbar / statusbar heights

These **scale with `font_size`** so a user who bumps the font slider doesn't get clipped chrome.

```rust
topbar_height(cfg)     // size_title + 2*SPACE_L
statusbar_height(cfg)  // size_body + 2*SPACE_S
```

### 1.7 Shadows

| Token | Use for |
|---|---|
| `SHADOW_CARD` | Raised cards on a surface |
| `SHADOW_POPUP` | Popovers (in-flow detail surfaces) |
| `SHADOW_WINDOW` | Modals (settings, about, save-template) |

### 1.8 Iconography

The app uses two icon families:

1. **Phosphor Regular** (via [`egui-phosphor`](https://crates.io/crates/egui-phosphor)) — every UI control glyph, status pictogram, and empty-state hero image. Registered in `app::install_fonts` as a font fallback so any `RichText` containing a phosphor codepoint renders inline. Reference glyphs through `widgets::phosphor::*` (e.g. `phosphor::ARROW_CLOCKWISE`, `phosphor::X`, `phosphor::PLAY`). **Never** use raw emoji glyphs (`↻`, `📚`, `⏳`, `✕`) — they render inconsistently across platforms.

2. **Race SVG badges** — `assets/race/{terran,protoss,zerg,random}.svg`, rendered through `widgets::race_badge`. These are brand marks, not generic UI iconography, so they live in the SVG image pipeline (not the icon font).

Icon size helpers in `tokens.rs` keep glyphs proportional to the surrounding text:

```rust
icon_size_caption(cfg) // 1.1 * size_caption
icon_size_body(cfg)    // 1.1 * size_body
icon_size_title(cfg)   // 1.1 * size_title
```

The 1.1× factor compensates for the optical weight difference between glyphs and surrounding text.

---

## 2. Components

Every component lives in `src/gui/widgets.rs`. The state matrices below cite the actual tokens applied — divergence from the matrix is a bug.

### 2.1 Chip — `chip(ui, label, selected, accent) -> Response`

Pill-shaped toggle. Used for filters, per-column toggles, overlays.

| State | Fill | Text | Notes |
|---|---|---|---|
| Default | `gray(40)` | `gray(160)` | `RADIUS_CHIP`, height ≥ `CHIP_MIN_HEIGHT` |
| Selected (no accent) | `rgb(55,75,55)` | `WHITE` | Muted green fallback |
| Selected (accent) | accent dimmed (1/3 + 20) | `WHITE` | Accent-tinted fill |
| Hover | egui default (handled by `Visuals.widgets.hovered`) | inherited | `HOVER_FILL` background |
| Disabled | n/a | n/a | Wrap in `add_enabled_ui(false, ...)` |

Sugar: `toggle_chip_bool(ui, label, &mut flag, accent)` toggles a bool on click.

### 2.2 Removable chip — `removable_chip(ui, label, cfg) -> Response`

Pill with label + trailing `×` (phosphor::X). The whole pill is the click target — clicking removes the filter.

| State | Fill | Border | Icon color |
|---|---|---|---|
| Default | `rgb(48,56,72)` | `1px gray(70)` | `gray(170)` |
| Hover | `rgb(78,50,56)` | `1px ACCENT_DANGER` | `ACCENT_DANGER` |
| Selected | n/a (one-state widget) | n/a | n/a |

### 2.3 Card — `card(ui, accent, |ui| ...) -> InnerResponse<R>`

Raised surface with hairline border and subtle shadow. Optional 3px left stripe in `accent` color.

| Element | Token |
|---|---|
| Fill | `CARD_FILL` |
| Border | `STROKE_HAIRLINE` × `BORDER` |
| Radius | `RADIUS_CARD` |
| Shadow | `SHADOW_CARD` |
| Padding | `CARD_INNER_MX` × `CARD_INNER_MY` |

### 2.4 Icon button — `icon_button(ui, glyph, hover) -> Response`

Chromeless small button for header affordances (back, reload, help, copy). Pass a phosphor glyph; the function delegates to `ui.small_button` so the standard `Visuals.widgets.{hovered,active}` matrix governs state colors.

### 2.5 Player identity — `player_identity(ui, name, race, idx, is_user, density, cfg, lang)`

The canonical "race badge · name · YOU?" composition shared by topbar, side panel, and build-order column header.

- **Name color**: `player_slot_color_bright(idx)` (slot is canonical)
- **Race badge**: SVG icon for T/P/Z/R, falls back to a colored text pill for unknown races
- **YOU chip**: appears when `is_user`, styled with `USER_CHIP_FG`/`USER_CHIP_BG` (teal-tinted, deliberately distinct from the slot color so it doesn't fight the name)

Density:
- `NameDensity::Compact` — caption-sized name + body-sized icon (single-line bars: topbar, chat row)
- `NameDensity::Normal` — subtitle-sized name + title-sized icon (card headers: side panel, build-order columns)

`NameDensity` is local to `player_identity` — there is no global density setting.

### 2.6 Player POV picker — `player_pov_pill` / `player_pov_selector`

Clickable pill showing race + name; used to pick which player a tab is "looking from" (Insights, Charts). Replaces ad-hoc ComboBoxes.

| State | Fill | Stroke |
|---|---|---|
| Default | `gray(36)` | `STROKE_HAIRLINE` × `BORDER` |
| Selected | slot color tinted (40 + slot/5) | `1.5px slot color` |

Sized via `PlayerPickerSize::{Small,Medium,Large,ExtraLarge}` — every dimension derives from typography tokens, so the widget scales with the font slider.

### 2.7 Labeled value — `labeled_value(ui, label, value)`

Two-column "key › value" row for popovers and compact key/value tables.

- Label: `.strong()` × `LABEL_STRONG`
- Separator: `›` in `gray(80)`
- Value: `LABEL_DIM`

### 2.8 Copy / reveal-in-explorer buttons

`copy_icon_button`, `copy_labeled_button`, `reveal_in_explorer_button_widget` use shared SVGs in `assets/icons/`. These are kept as SVGs (not phosphor) because the existing assets already match the app's stroke style and pre-date the phosphor adoption.

---

## 3. Patterns

### 3.1 Library — sidebar + list + detail

Three-zone composition:
- Filter sidebar (left) — `library/sidebar.rs`
- Virtualized entry list (center) — `library/ui.rs` + `library/entry_row.rs`
- Detail card (right, populated by selection) — `app/library_detail.rs`
- KPI hero strip (top, full width) — `library/hero.rs`

### 3.2 Analysis — topbar + tabs + side panel

- Topbar carries replay identity (map, matchup, players)
- Tab bar below, then a tab body composed of:
  - Side panel (per-player stats, dividers between blocks)
  - Main content (minimap / table / charts)

Blocks inside the side panel are separated by `ui.separator()` + `SPACE_XS`. No outer margins — relies on `Frame::inner_margin` with `CARD_INNER_*`.

### 3.3 Topbar / statusbar chrome

- Fill: `SURFACE_ALT`
- Inner margin: `Margin::symmetric(SPACE_M, SPACE_S)` for topbar; `Margin::symmetric(SPACE_M, SPACE_XS)` for statusbar
- Heights: `topbar_height(cfg)` / `statusbar_height(cfg)` (scale with `font_size`)

### 3.4 Insight grid (responsive masonry)

Two-pass layout in `tabs/insights/grid.rs`:
1. **Sizing pass** — render every card into an invisible `Ui` to measure heights
2. **Visible pass** — chunk cards into rows, broadcast row height via `ctx.data`, re-render with `set_min_height`

Result: cards in the same row align to the tallest card's height. Tokens: `INSIGHT_CARD_MIN_W`, `INSIGHT_COL_GAP`, `INSIGHT_MAX_COLS`.

---

## 4. Color usage rules

### Slot vs race — the canonical example

Two players in a 1v1, P1 = Terran (red slot) vs P2 = Zerg (blue slot):

| Element | Color | Why |
|---|---|---|
| P1's name in side panel | `player_slot_color_bright(0)` (red) | "Which player" — slot |
| Army value chart series for P1 | `player_slot_color_bright(0)` (red) | "Which player" — slot |
| Race badge next to P1's name | Terran SVG (rendered in race color via the SVG itself) | "Which faction" |
| `TvZ` matchup code in library row | `T` colored Terran-blue, `Z` colored Zerg-magenta | "Which faction" — both letters describe races |
| Race filter chip in library sidebar | Race color | The chip *is* the race |

This rule is invariant across all surfaces. If a new tab needs to show "this player did X", color by slot. If it needs to show "this faction does Y", color by race.

### Semantic accents

| Accent | Use when |
|---|---|
| `ACCENT_SUCCESS` | Win, positive delta, "completed" outcome |
| `ACCENT_WARNING` | Supply block, inactive state, pending action |
| `ACCENT_DANGER` | Loss, destructive action (remove filter, dismiss error), failed outcome |

Don't use semantic accents for player/race identity — they're reserved for state.

---

## 5. Localization & density

- All user-visible strings come from the locale system: `t(key, lang)` for static strings, `tf(key, lang, &[("var", &val)])` for formatted ones. Bundles live in `data/locale/{en,pt-BR}.txt`.
- Long Portuguese labels can stretch chips and rows. The chip widget uses `min_size(0, CHIP_MIN_HEIGHT)` so chips of varying label widths align vertically.
- The user's `font_size` slider scales every typography helper; control-sizing tokens (`CONTROL_HEIGHT_*`) and bar-height functions (`topbar_height`, `statusbar_height`) follow suit. There is no separate density toggle.

---

## 6. Migration notes

This document was introduced alongside a normalization pass that:

- Added `CONTROL_HEIGHT_S/M/L`, `ROW_INNER_MX/MY`, `ROW_RIGHT_ZONE_W`, `CHECKBOX_COL_W`, `FRAME_CHROME_V`, `topbar_height`, `statusbar_height`, `icon_size_*` to `tokens.rs`.
- Removed `TOPBAR_HEIGHT` and `STATUSBAR_HEIGHT` constants in favor of the font-aware functions.
- Migrated every emoji glyph in the GUI to `egui-phosphor` regular variant (reload, back, hamburger, hourglass, film reel, eye, lightning, prohibit, X, hard hat, sword, shield, buildings, warning, game controller, folder open, clipboard, chat circle).
- Switched chart series colors from race to slot (already in place; documented as the canonical rule).
- Normalized hardcoded paddings/margins in `entry_row.rs`, `build_order.rs`, `transport.rs`, `central.rs`, `status_bar.rs` to the spacing tokens.

When in doubt: read this file, then `tokens.rs`, then `widgets.rs`. If the answer isn't in any of them, that's a gap — add to the system rather than working around it.
