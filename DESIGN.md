---
version: alpha
name: SpeechTT Sensor

# ============================================================================
# COLORS  —  every entry maps to a --color-* CSS variable
# Required tokens must appear here. Optional tokens appear only when the
# designer overrides their stylesheet fallback.
# ============================================================================
colors:
  # Brand — primary and accent are required; secondary and tertiary are optional.
  brand:
    primary:   "#ea5514"   # --color-brand-primary   (record button, waveform, highlights)
    accent:    "#ff8a3d"   # --color-brand-accent    (live text, active glow)
    secondary: "#2a2a32"   # --color-brand-secondary (raised chrome, inactive bars)
    # tertiary: omitted → --color-brand-tertiary: var(--color-brand-primary)

  # Action — all four are required
  action:
    success: "#35c48b"     # --color-action-success  (saved, connected)
    info:    "#4d9dff"     # --color-action-info     (neutral notices)
    warning: "#f0a82e"     # --color-action-warning  (paused state)
    danger:  "#e5484d"     # --color-action-danger   (errors, stop)

  # Text  (drop-.base rule: colors.text.base -> --color-text)
  text:
    base:    "#f2efeb"     # --color-text
    accent:  "#ff8a3d"     # --color-text-accent (streaming / highlighted words)
    muted:   "#9c9aa3"     # --color-text-muted
    ondark:  "#f2efeb"     # --color-text-ondark

  # Surface  (drop-.base rule: colors.surface.base -> --color-surface)
  surface:
    base:  "#17171b"       # --color-surface      (floating panel body)
    alt:   "#202027"       # --color-surface-alt  (inset transcript well, inputs)
    dark:  "#0e0e11"       # --color-surface-dark (top info strip, modal backdrop)
    card:  "#1c1c22"       # --color-surface-card (settings card)

# ============================================================================
# TYPOGRAPHY  —  entries are objects;
# fontFamily, fontWeight, lineHeight, letterSpacing values MUST use existing
# preset values from design-tokens.css (CSS variables)
# ============================================================================
typography:
  base: {
    fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Helvetica Neue", Arial, sans-serif',
    fontWeight: "var(--font-weight-regular)",
    lineHeight: "var(--line-height-regular)"
  }
  display: {
    fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Helvetica Neue", Arial, sans-serif',
    fontWeight: "var(--font-weight-extrabold)",
    lineHeight: "var(--line-height-tight)",
    letterSpacing: "var(--letter-spacing-tight)"
  }
  mono: {
    fontFamily: 'ui-monospace, "DejaVu Sans Mono", "Courier New", monospace',
    fontWeight: "var(--font-weight-thin)",
    lineHeight: "var(--line-height-regular)",
    letterSpacing: "var(--letter-spacing-wide)"
  }

# ============================================================================
# ROUNDED  —  corner radius presets (all five are required)
# ============================================================================
rounded:
  none: "0"
  sm:   "0.375rem"
  md:   "0.75rem"
  lg:   "1.25rem"
  full: "100%"

# ============================================================================
# SPACING  —  spatial rhythm scale, 4px linear (all required)
# ============================================================================
spacing:
  xs:   "0.25rem"
  sm:   "0.5rem"
  md:   "0.75rem"
  base: "1rem"
  lg:   "1.25rem"
  xl:   "2rem"
  xxl:  "3rem"

# ============================================================================
# ELEVATION  —  custom top-level family (shadow presets; optional, default none)
# Tuned for a dark UI: deep, soft, slightly warm-neutral black shadows.
# ============================================================================
elevation:
  sm: "0 1px 2px rgba(0, 0, 0, 0.4), 0 2px 6px rgba(0, 0, 0, 0.25)"
  md: "0 4px 10px -2px rgba(0, 0, 0, 0.45), 0 10px 24px rgba(0, 0, 0, 0.35)"
  lg: "0 8px 20px -4px rgba(0, 0, 0, 0.5), 0 24px 48px rgba(0, 0, 0, 0.45)"

# ============================================================================
# BORDER  —  custom top-level family (border WIDTHS; all required)
# Values match the 124 preset. Border COLORS are not tokens — components
# pick them under `components`.
# ============================================================================
border:
  sm: "1px"
  md: "2px"
  lg: "4px"
---

## Overview

SpeechTT's interface is a small floating panel that lives above the user's
windows while they dictate. The visual language is **bold and branded**: a
near-black charcoal panel, warm off-white text, and one decisive, deep
"Agfa Sensor" orange that owns the record button and the live waveform. The
orange is the product's signature — everything else is quiet so the recording
state reads at a glance.

The widget has four states: **Idle**, **Recording**, **Paused**, and
**Settings**. Recording and Paused must be distinguishable from across the
room, so status is carried by color, label, dot, and waveform shape all at
once — never by color alone.

## Colors

- **Primary (`#ea5514`)** — Deep Agfa-Sensor orange. The record/stop button,
  the waveform bars, the active state ring, and brand marks. Text and icons on
  it use `--color-on-primary` (black on this orange, for AA contrast).
- **Accent (`#ff8a3d`)** — Brighter orange. Live/streaming text, focus glow,
  hover lift on the primary button. Not for body copy.
- **Secondary (`#2a2a32`)** — Slate charcoal. Inactive waveform bars, pressed
  chrome, dividers on raised surfaces.
- **Success (`#35c48b`)** — Transcript saved, provider connected.
- **Info (`#4d9dff`)** — Neutral notices and links inside settings.
- **Warning (`#f0a82e`)** — The **Paused** state: status dot, label, and the
  resume affordance. Amber is deliberately far from the recording orange so
  the two never blur together.
- **Danger (`#e5484d`)** — Errors, failed transcription, destructive actions.
- **Text (`#f2efeb`)** — Warm off-white body text on dark surfaces.
- **Text accent (`#ff8a3d`)** — Newly arrived transcript words, highlighted
  selections. Never body copy.
- **Text muted (`#9c9aa3`)** — Shortcut hints, timestamps, secondary labels.
- **Surface base (`#17171b`)** — The floating panel body.
- **Surface alt (`#202027`)** — Inset transcript well, input fields.
- **Surface dark (`#0e0e11`)** — Top info strip and settings backdrop.
- **Surface card (`#1c1c22`)** — Settings cards and grouped rows.

### Resolved color contrast pairs

Every colored role above has a companion `--color-on-*` token: the foreground
to use for text or icons drawn on that role's background. They are
**auto-calculated**, not authored — a tool picks the theme text color with the
higher WCAG 2.1 contrast, or black/white when neither meets 4.5:1. They never
appear in the front matter. Components **must** use them for any text or icon
on a colored surface: the record button, status pill, toast, and all settings
buttons.

### Color variants

This [color variants companion stylesheet](./color-variants.css) can be used to
gain `*-muted` and `*-active` color variants derived from the theme's brand and
action colors:

- `muted` — `hsl(from <base> h calc(s * 0.8) calc(l * 1.2))` — less saturated,
  lighter; the softened, resting variant.
- `active` — `hsl(from <base> h calc(s * 1.2) calc(l * 1.1))` — more saturated,
  lighter; the vivid hover/pressed variant.

**Note:** Only `brand` and `action` colors carry these variants, because they
are intended to be used on interactive elements with _active/disabled_ states.

## Typography

Body copy, labels, and headings share the host system sans stack
(`system-ui`) so the widget renders instantly, offline, with zero font
requests — appropriate for a desktop overlay that must appear without a flash
of unstyled text. Brand boldness comes from weight and scale, not from a
downloaded face.

The type scale is geometric with ratio 1.25 (Major Third):
`size(step) = 1rem * 1.25^step`, so `md` is `1rem`, `lg` `1.25rem`, `xl`
`1.5625rem`, and `2xl` `1.953rem`. Shortcut hints use the `mono` style with
wide letter-spacing and uppercase labels. Font sizes are defined in the
stylesheet only — they are not front-matter entries.

## Layout

A 4px linear spacing scale with `1rem` (16px) as the base step. The floating
panel opens at 600 x 460 px (or the size and position remembered from the last
run) with `--rounded-lg` corners and `--space-base` internal padding. It can
never be resized below 600 x 360 px: that is the smallest box that fits the
mode chooser's two 272 px cards, the gap between them and the body inset, so
the cards never collide with the welcome title and both descriptions stay on
two lines. A remembered smaller geometry is
raised to that minimum on launch. The
width fits the mode chooser's two cards side by side; the height fits
both the chooser and the two-row setup form. The top info strip (shortcuts +
settings) is `surface-dark`; the transcript well is `surface-alt`; the round
actions sit on `surface`. The settings panel is a larger card
(`surface-card`) that opens beside the widget, not inside it.

## Elevation & Depth

The panel floats, so elevation is real: `--elevation-lg` on the panel itself
(a tight key shadow plus a wide ambient one), `--elevation-md` on the settings
panel, `--elevation-sm` on inset buttons and the status pill. On the dark
charcoal background the shadows read as depth, never as grime.

## Shapes

Corner radii: `0.375rem` for inputs, chips, and shortcut keycaps; `0.75rem` for
buttons and small cards; `1.25rem` for the floating panel and settings modal;
`100%` for the record button, status dot, and icon buttons. Borders are
1px for dividers and hairline edges, 2px for focus rings, 4px for the
drag indicator.

## Components

### Round button

Rendered by `components::RoundButton` — never a raw `div` sized at the call
site. Exactly one diameter (56 px, `rounded.full`), two tones:

- `Primary`: `brand.primary` fill, `--color-on-primary` icon, hover
  `brand.primary-active`.
- `Secondary`: `surface` fill, 1px `brand.secondary` border, `text` icon,
  hover `surface-alt-active` with a `brand.primary/40` border.

A click expands a single 2px ring behind the button from 56 to 84 px while
fading from 55% to 0% opacity over 400 ms (`ease-out-quint`). The ring never
affects layout, and it is driven by the view's monotonic pulse counter so it
restarts on every click. Inside a [`Card`] the round button is visual only —
it has no hover, cursor or click — because the card owns the interaction.

### Card

Rendered by `components::Card`: `surface-alt`, `rounded-md`, `--space-lg`
padding, `--space-md` gaps, 1px `brand.secondary` border. Cards fill their row
by default; `Card::width` pins one to a fixed width for rows that must keep
their intrinsic size. Interactive cards
show a pointer cursor, a `brand.primary/50` hover border and a
`surface-button-active` pressed fill. Cards own their click, so a whole card
(title, description, ring, caption) is one target.

### Mode chooser

The `Blank` screen: no status pill, `--space-lg` inset, title **Choose a
mode** at `--font-size-lg` semibold, subtitle at `--font-size-sm` muted, then
two equal cards (live dictation, from a file). Both cards are pinned to
`MODE_CARD_WIDTH` (216 px), sit `--space-base` apart and are centered as a
pair, so extra window space grows around the pair, never inside the cards.
Each card centres a title
(`--font-size-md` semibold), a wrapped description (`--font-size-sm`
muted), a round button and an uppercase-free `--font-size-xs` medium caption
(`Start`, `Browse files`). Captions align because the cards are the same
height.

### Record / transport controls

The round button is the hero of the recording screens at the same 56 px size
as everywhere else: `Primary` with a mic glyph on Ready, a pause glyph while
recording, a play glyph on Paused and a stop square (small square
`IconAction`) beside them. Paused's Resume stays `Primary`; colour alone
never distinguishes the states — the status pill and waveform do.

### Waveform

32 rounded bars, 3px wide, `rounded.full`, gap `--space-xs`. Bar height maps
to the FFT magnitude of its frequency band, from `--space-xs` (silence) to
`--space-xl` (peak). Recording bars are `brand.primary` with the newest few at
`brand.accent`. Paused bars drop to `brand.secondary` at a flat, low height so
the panel visibly exhales.

### Transcript well

Inset `surface-alt` panel, `rounded-md`, `space-md` padding, `elevation-sm`.
Body text in `text` at `--font-size-sm`, line-height `regular`. Arriving words
are briefly `text.accent`, settling to `text` after ~600ms. Auto-scrolls to
the newest line; a soft fade at the top edge hints at earlier text.

### Status pill

Small pill (`rounded.full`) at the top-left of the transcript section: a 8px
dot plus an uppercase `mono` label at `--font-size-xs`,
`--letter-spacing-wide`. Recording = `brand.primary` dot with pulse + label
`RECORDING`; Paused = `action.warning` dot, static + label `PAUSED`; Idle =
`text.muted` dot + label `READY`.

### Action buttons

Rendered by `components::ActionButton`, never a raw `Button` with a size set
at the call site. One medium size everywhere: `--font-size-md`, 32px high,
`--space-sm`/`--space-md` horizontal padding. `Primary` marks the single
committing action in a group (Save, Continue, Got it); `Secondary` marks the
dismissing or alternative action (Cancel, Back, Change mode). No other tones are needed.

### Icon buttons

Rendered by `components::IconButton`: 32px ghost targets in the window header
and dialog close slots, icon in `text`/`text.muted` with a `brand.primary`
hover tint. Always include an accessible label; shortcuts are displayed in
the top strip. Dense inline controls inside the widget (Reveal, select
carets, "New take") may stay small and are not action buttons.

### Shortcut hints

`mono`, `--font-size-xs`, `text.muted`, uppercase keycaps in
`surface-alt` with `rounded-sm` and 1px `brand.secondary`. Copy explains the
cross-platform binding (e.g. `Ctrl+Shift+R`) so the strip reads as
documentation, not decoration.

### Settings panel

A 560px-wide card in `surface-card`, `rounded-lg`, `elevation-md`, with rows
separated by 1px `brand.secondary` hairlines. Provider select, masked API key
field, default model select, and save/cancel actions. Labels in `mono`
uppercase `--font-size-xs`; values in `base` `--font-size-sm`.

## DO's and DON'Ts

- Do let the filled primary round button be the only saturated shape on a
  screen.
- Do signal Recording vs Paused with dot + label + waveform shape, not color alone.
- Do use `--color-on-*` for anything drawn on a brand or action background.
- Do keep shortcut reminders visible in the top strip while recording.
- Do not use `text.accent` for body copy — it is for newly arrived words only.
- Do not use the recording orange for the paused state; amber owns pause.
- Do not introduce ad-hoc colors, radii, or shadows outside this token set.
