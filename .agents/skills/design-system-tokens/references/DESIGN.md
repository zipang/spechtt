---
version: alpha
name: Reference Design System

# ============================================================================
# COLORS  —  every entry maps to a --color-* CSS variable
# Required tokens must appear here. Optional tokens appear only when the
# designer overrides their stylesheet fallback.
# ============================================================================
colors:
  # Brand — primary and accent are required; secondary and tertiary are
  # optional.
  brand:
    primary:   "#000000"   # --color-brand-primary   (headings, logo, contrast)
    accent:    "#ffcc00"   # --color-brand-accent    (CTAs, highlights, selection)
    secondary: "#333333"   # --color-brand-secondary (optional, overridden)
    # tertiary: omitted → --color-brand-tertiary: var(--color-brand-primary)

  # Action — all four are required
  action:
    success: "#2e7d4f"     # --color-action-success
    info:    "#2a6f97"     # --color-action-info
    warning: "#b7791f"     # --color-action-warning
    danger:  "#b23a2e"     # --color-action-danger

  # Text  (drop-.base rule: colors.text.base -> --color-text)
  text:
    base:    "#000000"     # --color-text
    accent:  "#ffcc00"     # --color-text-accent (highlighted or selected text)
    muted:   "#606060"     # --color-text-muted
    ondark:  "#ffffff"     # --color-text-ondark

  # Surface  (drop-.base rule: colors.surface.base -> --color-surface)
  surface:
    base: "#ffffff"        # --color-surface
    alt:  "#f2f2f2"        # --color-surface-alt
    dark: "#0f0f0f"        # --color-surface-dark      (optional, overridden)
    # card:  omitted → --color-surface-card: var(--color-surface)

# ============================================================================
# TYPOGRAPHY  —  entries are objects;
# fontFamily, fontWeight, lineHeight, letterSpacing values MUST use existing preset values from design-tokens.css (CSS variables)
# ============================================================================
typography:
  # Font families — No font size
  base: {
    fontFamily: 'ui-serif, "Palatino Linotype", Cambria, Georgia, serif',
    fontWeight: "var(--font-weight-regular)",
    lineHeight: "var(--line-height-regular)"
  }
  display: {
    fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Helvetica, Verdana, sans-serif',
    fontWeight: "var(--font-weight-bold)",
    lineHeight: "var(--line-height-tight)",
    letterSpacing: "var(--letter-spacing-tight)"
  }
  mono: {
    fontFamily: 'ui-monospace, "Courier New", monospace',
    fontWeight: "var(--font-weight-thin)",
    lineHeight: "var(--line-height-regular)"
  }

# ============================================================================
# ROUNDED  —  corner radius presets (all five are required)
# Values in rem, except full (100% for circular shapes).
# ============================================================================
rounded:
  none: "0"
  sm:   "0.25rem"
  md:   "0.75rem"
  lg:   "1.125rem"
  full: "100%"   # circular — avatars, icons, pills

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
# Values match the flat preset (presets/elevation/flat.css).
# ============================================================================
elevation:
  sm: "none"
  md: "none"
  lg: "none"

# ============================================================================
# BORDER  —  custom top-level family (border WIDTHS; all required)
# Values match the 124 preset (presets/borders/124.css).
# Border COLORS are not tokens — components pick them under `components`.
# ============================================================================
border:
  sm: "1px"
  md: "2px"
  lg: "4px"
---

## Overview

This is a reference DESIGN.md demonstrating the dual-file token contract from
the `design-system-tokens` skill. It describes a minimalist, editorial theme
inspired by National Geographic: black type, white paper, and one yellow accent.
The front matter above lists every required token plus a selection of optional
tokens. Optional tokens that are omitted here still exist in the stylesheet with
a `var()` fallback to a required token.

## Colors

- **Primary (`#000000`)** — Black. Headings, logo, and contrast.
- **Accent (`#ffcc00`)** — National Geographic yellow. CTAs, highlights, and
  selection. Use it for surfaces and marks, not for body text.
- **Secondary (`#333333`)** — Dark gray. Additional brand color for decorative
  surfaces.
- **Tertiary** — omitted; the stylesheet falls back to `var(--color-brand-primary)`.
- **Success (`#2e7d4f`)** — Forest green. Form success, confirmation.
- **Info (`#2a6f97`)** — Slate blue. Informational messages.
- **Warning (`#b7791f`)** — Ochre. Warnings, cautionary messages.
- **Danger (`#b23a2e`)** — Brick red. Errors, destructive actions.
- **Text base (`#000000`)** — Body text.
- **Text accent (`#ffcc00`)** — Highlighted or selected text. It maps to the
  brand accent, so it is yellow/orange. Do not use it for body copy, links, or
  headings.
- **Text muted (`#606060`)** — Subdued text: captions, notes.
- **Text ondark (`#ffffff`)** — Text on dark surfaces: footer, dark sections.
- **Surface base (`#ffffff`)** — Default page background.
- **Surface alt (`#f2f2f2`)** — Alternating sections and aside panels.
- **Surface dark (`#0f0f0f`)** — Footer, dark sections.
- **Surface card** — omitted; falls back to `var(--color-surface)`.

### Resolved color contrast pairs

Every colored role above has a companion `--color-on-*` token: the foreground to use
for text drawn on that role's background. They are **auto-calculated**, not authored —
a tool picks the theme text color with the higher WCAG 2.1 contrast, or black/white when
neither meets 4.5:1. They never appear in the front matter. UI components must use them
for any text on a surface: the `primary`, `accent`, `success`, `warning`, `danger`, and
`info` button variants, the avatar, and the send button all read their foreground from
`--color-on-*`.

### Color variants

This [color variants companion stylesheet](./color-variants.css) can be used to 
gain `*-muted` and `*-active` color variants derived from the theme's brand and action colors:

The calculation is made with:
- `muted`  — `hsl(from <base> h calc(s * 0.8) calc(l * 1.2))` — less
  saturated, lighter; the softened, resting variant.
- `active` — `hsl(from <base> h calc(s * 1.2) calc(l * 1.1))` — more
  saturated, lighter; the vivid hover/pressed variant.

**Note:** Only `brand` and `action` colors carry these variants, because they are 
intended to be used on interactive elements with _active/disabled_ states. 

## Typography

Body text uses a system serif stack (`ui-serif`, Palatino Linotype, Cambria,
Georgia). Headings use the system sans stack for an editorial contrast. Code and
labels use `ui-monospace`. The type scale is geometric with ratio 1.5 (Perfect
Fifth): `size(step) = 1rem * 1.5^step`, so `md` is `1rem`, `lg` `1.5rem`, `xl`
`2.25rem`, and `xs`/`sm` go down to `0.444rem`/`0.667rem`. Font sizes are
defined in the stylesheet only — they are not front-matter entries. The
optional `2xl` and `display` steps fall back to `var(--font-size-xl)` unless
overridden in the stylesheet.

## Layout

A 4px linear spacing scale with 1rem (16px) as the base step. Content is
constrained to a max-width of 1200px with generous section padding.

## Elevation & Depth

The design is flat — all elevation tokens are set to `none` (flat preset).
Depth is conveyed through color contrast and spacing rather than shadows.

## Shapes

Corner radii range from 4px (0.25rem — chips, inputs) to 100% (circular
avatars). Cards and thumbnails use 0.75rem. Pill buttons and large chips use
1.125rem.

## Components

Components pick the color tokens they need for their states and variants.

### Link

Links are black and underlined. On hover and active, they get a pale yellow
highlight derived from the brand accent. Visited links use the muted text color.

- **default** — `textColor: "{colors.text.base}"` → `--color-text`
- **hover and active** — highlighted with `--color-brand-accent-muted` (the
  softened accent variant)
- **visited** — `textColor: "{colors.text.muted}"` → `--color-text-muted`

In the stylesheet this maps to:

```css
a {
    color: var(--color-text);
    text-decoration: underline;
}
a:hover,
a:active {
    background: var(--color-brand-accent-muted);
}
a:visited {
    color: var(--color-text-muted);
}
```

The same pattern applies to other components that need specific colors for
their variants — e.g. a Card component may pick `colors.surface` for its
background and `colors.brand.accent` for a highlight bar.

## DO's and DON'Ts

- Do reserve the accent yellow for highlights, selection, and CTAs
- Do use `var()` fallbacks for optional tokens in the stylesheet
- Do use the black primary color for text and links
- Do use `colors.text.accent` only for highlighted or selected text, never for
  body copy
- Do not list the derived `muted` / `active` variants as tokens in the front
  matter — they are generated in `color-variants.css`
