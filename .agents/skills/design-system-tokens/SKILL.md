---
name: design-system-tokens
description: The Design System is a dual-file contract (DESIGN.md + design-tokens.css) of fixed design tokens exposed as CSS variables. Use this skill to create or update the design token values, or to document their usage to create components.
---

# 1. Overview

This skill documents the usage of design tokens in our Design System Applied approach.

## The dual-file contract

The Design System is composed of two main files:

- **`DESIGN.md`**: The agent-readable documentation of your design system. This markdown file is divided into two parts: the _design tokens_ living inside the YAML front-matter, and the prose sections documenting their usage to create components. The full specifications for this file can be found here: [google-labs-code/design.md](https://github.com/google-labs-code/design.md).
- **`design-tokens.css`**: A stylesheet containing all the design tokens from our `DESIGN.md` front matter exposed as `:root` (global) CSS variables.

## New Rules

Our _Applied Design System_ approach adds new rules that are not found in the original `DESIGN.md` spec. Here they are:

- The full list of usable design tokens is **fixed** and documented.
- Do not add any new undocumented token.
- To change a token value: edit the value in **both** `DESIGN.md` front matter and `design-tokens.css`. Keep them identical.
- Design tokens inside `DESIGN.md` front matter are structured into categories and sub-categories that make them accessible by their path.
- Token categories are: `colors` (brand, action, text, surface), `typography` (base, display, mono families; scale xs→display; weights; line heights; letter spacing), `spacing`, `rounded`, `elevation`, `border`.
- Design tokens re-exposed as CSS variables are flattened, so we need a mapping between the two notations. The tables in sections 2–7 are the source of truth for that mapping.
- Mapping classes:
  - **Front-matter-mapped tokens** have a `Token path` and a `CSS variable`, for example `colors.text.base` → `--color-text` (drop `.base`) or `typography.base.fontFamily` → `--font-family-base`. The table row defines the mapping; do not derive it mechanically.
  - **Stylesheet-only tokens** have no `Token path` (the font-size, font-weight, line-height, and letter-spacing scales). Define them directly in `design-tokens.css`; they do not appear in the front matter.
- Optional tokens are always defined in `design-tokens.css`. They fall back to a `var()` reference to a required token, or to a documented literal value (for example `--elevation-sm: none`).
- All size values must be given in `rem` units.
- A `--font-family-*` token names a font. It does not load one. When a family is a hosted web font, `design-tokens.css` must import it. Use an `@import` at the top of the file, or a matching `@font-face`. Without the import, the token falls back to the local stack.

The next sections will now introduce the full list of available tokens:

# 2. Typography

Define font family and give each font a semantic role inside: `base` (body), `display` (headings), `mono` (code, labels).

## Font families

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `typography.base.fontFamily` | `--font-family-base` | Y | — | Body text and general UI font |
| `typography.display.fontFamily` | `--font-family-display` | Y | — | Headings and large display text |
| `typography.mono.fontFamily` | `--font-family-mono` | N | `var(--font-family-base)` | Code, labels, monospaced content |

### Hosted web fonts

A `--font-family-*` value is only a stack of names. The browser fetches nothing. When the
stack names a hosted web font, the stylesheet must load it. If it does not, the token
resolves to the fallback. The theme font does not apply.

- Load it from the stylesheet itself. Put the `@import` at the very top of
  `design-tokens.css`, before the `:root` block and every other rule. CSS honors
  `@import` only before other rules.
- Request the weights the theme uses, from `--font-weight-*` (for example `400;500;700`),
  so no weight is synthesized.
- A `@font-face` block or another host's stylesheet is also valid. The stylesheet must
  not depend on the page to load the font.
- System stacks (`ui-serif, Georgia, serif`, …) name only local fonts and need no import.

```css
@import url("https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;700&family=DM+Serif+Display:wght@400;700&display=swap");

:root {
	/* … tokens … */
}
```

## Apply a typographic scale

**Principle:** Use a geometric scale (e.g. `1.125` Minor Second, `1.25` Major Third, `1.333` Perfect Fourth, `1.5` Perfect Fifth, `1.618` Golden Ratio) or a linear scale (e.g. `2px` steps) to generate the values from `xs` through `display`: `[xs, sm, md, lg, xl, 2xl, display]`
**Formula:**
  - For a geometric scale with ratio `r` the size of each step is `size(step) = 1rem * r^step`.
  - For a linear scale with step `r` the size of each step is `size(step) = 1rem + r*step`.
**Base:** The root base font size (`--font-size-base`) is always equal to `1rem` and is the step 0. It is usually applied to size `md` (`--font-size-md`). Step 0 can also be applied to `sm` or `lg` for specific use cases.


The font-size, font-weight, line-height, and letter-spacing tokens have **no front-matter
path**: they are **stylesheet-only** tokens, defined directly in `design-tokens.css`. An
empty `Token path` cell marks this class.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
|   | `--font-size-base` | Y | — | Root base size, fixed `1rem` (step 0) |
|   | `--font-size-xs` | N | `var(--font-size-sm)` | Captions, metadata, timestamps |
|   | `--font-size-sm` | Y | — | Secondary text, list rows |
|   | `--font-size-md` | Y | — | Body text |
|   | `--font-size-lg` | Y | — | Section titles, card titles |
|   | `--font-size-xl` | Y | — | Page titles, large prompts |
|   | `--font-size-2xl` | Y | — | Hero titles, large headings |
|   | `--font-size-display` | N | `var(--font-size-xl)` | Hero / banner headlines |

## Font weights

The only available weights are the four below. How they combine with a font family, line
height, and letter spacing is defined by the three typographic styles — `base`,
`display`, and `mono` — in `DESIGN.md`.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
|  | `--font-weight-thin`      | Y | — | Lightest — the `mono` (UI) style |
|  | `--font-weight-regular`   | Y | — | Default — the `base` (body) style |
|  | `--font-weight-bold`      | Y | — | Strong — the `display` (headings) style |
|  | `--font-weight-extrabold` | N | `var(--font-weight-bold)` | Optional stronger emphasis |

## Line heights

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
|  | `--line-height-tight`   | Y | — | Headings, single-line titles |
|  | `--line-height-regular` | Y | — | Body text, multi-line descriptions |
|  | `--line-height-relaxed` | N | `var(--line-height-regular)` | Long-form reading, comments |

## Letter spacing

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
|  | `--letter-spacing-tight` | Y | — | Large headings, display text |
|  | `--letter-spacing-regular` | Y | — | Body text, default tracking |
|  | `--letter-spacing-wide` | N | `var(--letter-spacing-regular)` | Small labels, metadata |

# 3. Colors

The color palette is divided into four categories: `brand`, `action`, `text`, and `surface`.

Link and border *colors* are **not** tokens — they are not part of the palette. Individual components pick the color tokens they need for their states (see the Link component example in section 8).

## Brand colors

The first and most important colors. At minimum a `colors.brand.primary` and a `colors.brand.accent` are required. Optional additions: `colors.brand.secondary`, `colors.brand.tertiary`.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `colors.brand.accent` | `--color-brand-accent` | Y | — | Brand accent color — CTAs, key accents |
| `colors.brand.primary` | `--color-brand-primary` | Y | — | Primary brand color — headings, highlights, contrast, logo, header |
| `colors.brand.secondary` | `--color-brand-secondary` | N | `var(--color-brand-primary)` | Additional brand color — for surfaces |
| `colors.brand.tertiary` | `--color-brand-tertiary` | N | `var(--color-brand-primary)` | Additional brand color — for surfaces |

## Action colors

Semantic colors for feedback states.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `colors.action.success` | `--color-action-success` | Y | — | Success states, confirmations, positive feedback |
| `colors.action.info` | `--color-action-info` | Y | — | Informational messages, neutral notifications |
| `colors.action.warning` | `--color-action-warning` | Y | — | Warnings, cautionary messages |
| `colors.action.danger` | `--color-action-danger` | Y | — | Errors, destructive actions, critical alerts |

## Text colors

Only `colors.text.base` is required.
The variants are optional and cover hierarchy and emphasis. If no value is found, each variant falls back to its stylesheet fallback.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `colors.text.base` | `--color-text` | Y | — | Default body text color (`<p>`) |
| `colors.text.accent` | `--color-text-accent` | N | `var(--color-text)` | Accented text — links, headings, highlighted text |
| `colors.text.muted` | `--color-text-muted` | N | `var(--color-text)` | Subdued text — visited links, notes |
| `colors.text.ondark` | `--color-text-ondark` | N | `var(--color-surface)` | Text on dark surfaces — footer, dark sections |

## Surface colors

Background colors for surfaces like page sections or cards.
At minimum two surfaces are required: `colors.surface.base` (exposed as `--color-surface`) and `colors.surface.alt`.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `colors.surface.base` | `--color-surface` | Y | — | Default page / section background |
| `colors.surface.alt` | `--color-surface-alt` | Y | — | Alternating section backgrounds |
| `colors.surface.dark` | `--color-surface-dark` | N | `var(--color-text)` | Dark surfaces — footer, dark sections |
| `colors.surface.card` | `--color-surface-card` | N | `var(--color-surface)` | Card, forms surfaces |

## Resolved color contrast pairs

Every colored role has a companion **`--color-on-*`** token: the foreground to use for text
drawn on that role's background. Unlike the palette tokens, these are **derived, not
authored**. A tool recomputes them with WCAG 2.1 contrast, so a theme cannot place an
illegible label on a colored surface.

They are **stylesheet-only** tokens: no front-matter path, no entry in `DESIGN.md` front
matter. Every one is present in `design-tokens.css` and defaults to `var(--color-text)`.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
|   | `--color-on-primary` | N | `var(--color-text)` | Text on `colors.brand.primary` |
|   | `--color-on-accent` | N | `var(--color-text)` | Text on `colors.brand.accent` |
|   | `--color-on-secondary` | N | `var(--color-text)` | Text on `colors.brand.secondary` |
|   | `--color-on-tertiary` | N | `var(--color-text)` | Text on `colors.brand.tertiary` |
|   | `--color-on-success` | N | `var(--color-text)` | Text on `colors.action.success` |
|   | `--color-on-info` | N | `var(--color-text)` | Text on `colors.action.info` |
|   | `--color-on-warning` | N | `var(--color-text)` | Text on `colors.action.warning` |
|   | `--color-on-danger` | N | `var(--color-text)` | Text on `colors.action.danger` |
|   | `--color-on-dark` | N | `var(--color-text)` | Text on `colors.surface.dark` |
|   | `--color-on-alt` | N | `var(--color-text)` | Text on `colors.surface.alt` |
|   | `--color-on-card` | N | `var(--color-text)` | Text on `colors.surface.card` |

**Derivation:** for each role, resolve the background and the theme's two text colors
(`--color-text`, `--color-text-ondark`) to hex, then pick the text color with the higher
WCAG 2.1 contrast ratio. A tie goes to `--color-text-ondark`. When the winning ratio is
below 4.5:1, fall back to pure black or white, whichever contrasts more. A background in a
non-hex notation (`rgba(...)`, `color-mix(...)`) leaves the token at its default.

**Base surface:** `colors.surface.base` has no `--color-on-*` companion. The rule is that
`--color-text` is the appropriate contrasting color for `--color-surface`, and the tool
checks that this pair meets 4.5:1.

**Component rule:** UI components must use these tokens for any text drawn on that role's
background, for example the `primary`, `accent`, `success`, `warning`, `danger`, and `info`
button variants. `DESIGN.md` documents the rule in prose (see section 8).

## Color variants derivation

Brand and action colors carry `muted` and `active` variants that are **derived automatically** from the base color via CSS relative color syntax. These variants are **not design tokens** — they do not appear in the front matter. They live in a dedicated [`color-variants.css`](./references/color-variants.css) file, included after the main design-tokens stylesheet.

**Derivation rules:**

- **`muted`** — less saturated, lighter: the softened, resting variant.
  ```css
  --color-brand-primary-muted: hsl(from var(--color-brand-primary) h calc(s * 0.8) calc(l * 1.2));
  ```
- **`active`** — more saturated, lighter: the vivid variant for hover/pressed states.
  ```css
  --color-brand-primary-active: hsl(from var(--color-brand-primary) h calc(s * 1.2) calc(l * 1.1));
  ```

Only `brand` and `action` colors carry these variants, because they are used on interactive elements with states. Text and surface colors define their own variants explicitly in the tables above.

**In `DESIGN.md`** — add a `### Color variants` subsection under `## Colors` documenting the derivation rule in prose, so agents know the variants exist and how they are produced. Do not list the variants as tokens in the front matter.

# 4. Spacing

Choose a linear (e.g. `4px` steps) or a geometric scale (`1.5` or `2` ratio) to create the full series from `spacing.xs` to `spacing.xxl`.

**Note:** `spacing.base` is the step 0 and is always equal to `1rem`. All values are in `rem` units.

The formula for a linear scale with step `s` (in px) is: `space(step) = s * step`, where `xs` = step 1 (4px), `sm` = step 2 (8px), `md` = step 3 (12px), `lg` = step 5 (20px), `xl` = step 8 (32px), `xxl` = step 12 (48px).

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `spacing.xs` | `--space-xs` | Y | — | Tight gaps, icon-to-text spacing |
| `spacing.sm` | `--space-sm` | Y | — | Compact gaps, chip padding |
| `spacing.md` | `--space-md` | Y | — | Button padding, card internal spacing |
| `spacing.base` | `--space-base` | Y | — | Default spacing, input padding (step 0 = 1rem) |
| `spacing.lg` | `--space-lg` | Y | — | Section internal spacing, larger card gaps |
| `spacing.xl` | `--space-xl` | Y | — | Section-to-section spacing |
| `spacing.xxl` | `--space-xxl` | Y | — | Page-level vertical rhythm, section padding |

# 5. Rounded

Corner radius presets for buttons, cards, inputs, and other rectangular shapes.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `rounded.none` | `--rounded-none` | Y | — | Square shapes — fixed value `0` |
| `rounded.sm` | `--rounded-sm` | Y | — | Inputs, small badges, default controls |
| `rounded.md` | `--rounded-md` | Y | — | Cards, buttons, default containers |
| `rounded.lg` | `--rounded-lg` | Y | — | Large panels, prominent cards |
| `rounded.full` | `--rounded-full` | Y | — | Circular shapes — avatars, icons, pills; fixed value `9999px` |

# 6. Elevation

Elevation defines the shadow vocabulary. The stylesheet sets `--elevation-*` directly, so it stands alone; the presets below are optional recipes you can copy from.

| Token path | CSS variable | Required | Default (flat) | Description |
|---|---|---|---|---|
| `elevation.sm` | `--elevation-sm` | N | `none` | Subtle depth — cards, default raised elements |
| `elevation.md` | `--elevation-md` | N | `none` | Moderate depth — raised cards, modals |
| `elevation.lg` | `--elevation-lg` | N | `none` | Strong depth — dropdowns, popovers, overlays |

Available presets (given as examples, they are not the only options):

- [Flat Design](./presets/elevation/flat.css) — all elevations set to `none`
- [Brutal shadows](./presets/elevation/brutal.css) — solid rectangular offset shadows
- [Paper](./presets/elevation/paper.css) — soft, realistic, ambient + key shadows
- [Neumorphism](./presets/elevation/neumorphism.css) — soft dual light/dark shadows for an extruded-surface feel

# 7. Borders

Border *width* defines the stroke thickness vocabulary. The stylesheet sets `--border-*` directly, so it stands alone; the presets below are optional recipes you can copy from. `border` is a **custom top-level family** in the front matter. Border *colors* are not tokens — components pick the color tokens they need for their border variants.

| Token path | CSS variable | Required | Default | Description |
|---|---|---|---|---|
| `border.sm` | `--border-sm` | Y | — | Default stroke — dividers, input borders |
| `border.md` | `--border-md` | Y | — | Emphasis stroke — focus rings, active selection |
| `border.lg` | `--border-lg` | Y | — | Strong emphasis — drag indicators, prominent outlines |

Available presets:

- [None](./presets/borders/none.css) — all borders set to `none`
- [124](./presets/borders/124.css) — 1px / 2px / 4px
- [macOS](./presets/borders/macos.css) — hairline 0.5px separators, 1px fields, 2px focus
- [Windows](./presets/borders/windows.css) — 1px chrome, 2px accent focus, 4px input borders

# 8. DESIGN.md example

A complete, annotated example of a `DESIGN.md` file — front matter plus prose sections — is provided in [references/DESIGN.md](./references/DESIGN.md).

It demonstrates:

- Every **required token** defined in the front matter, structured under Google's top-level families (`colors`, `typography`, `rounded`, `spacing`) plus the two custom families (`elevation`, `border`).
- A selection of **optional tokens** overridden by the designer (e.g. `colors.brand.secondary`, `colors.text.muted`).
- **Optional tokens omitted** from the front matter with inline comments showing the stylesheet fallback (e.g. `# tertiary: omitted → --color-brand-tertiary: var(--color-brand-primary)`).
- The `components:` block with a **Link component** example showing how components pick color tokens for their states.
- The `### Color variants` prose subsection documenting the derived `muted` / `active` rule.
- The `### Resolved color contrast pairs` prose subsection documenting the derived
  `--color-on-*` tokens and the rule that components use them for text on a surface.
- All prose sections in the canonical Google order: Overview, Colors, Typography, Layout, Elevation & Depth, Shapes, Components, Do's and Don'ts.

Inline comments in the front matter map each entry back to its CSS variable in the stylesheet.

# 9. Reference files

| File | Purpose |
|------|---------|
| [references/DESIGN.md](./references/DESIGN.md) | Complete annotated example of a `DESIGN.md` file — front matter plus prose sections. Demonstrates required and optional tokens, stylesheet fallbacks, and the Link component example. |
| [references/design-tokens.css](./references/design-tokens.css) | Complete example of the design-tokens stylesheet. Exposes every token defined in sections 2–7 as a CSS variable inside a single `:root` block. The reference theme uses system fonts, so it needs no import. A theme with a hosted web font must open the file with one (section 2). |
| [references/color-variants.css](./references/color-variants.css) | Derived `muted` / `active` variants for brand and action colors. Include **after** the main design-tokens stylesheet. Not part of the token set. |
| [references/reset.css](./references/reset.css) | Base CSS reset consuming the theme variables. |
| [references/utilities.css](./references/utilities.css) | Class-based utilities to apply the theme variables in a Tailwind fashion. |
| [presets/elevation/](./presets/elevation/) | Optional elevation recipes: `flat.css`, `brutal.css`, `paper.css`, `neumorphism.css`. |
| [presets/borders/](./presets/borders/) | Optional border width recipes: `none.css`, `124.css`, `macos.css`, `windows.css`. |

# 10. Validation rules

Check each of these rules after any edit to the Design System files:

* **Undocumented tokens are FORBIDDEN** in the stylesheet or in the front matter. Contrary to the Google Labs approach, only the tokens listed in sections 2–7 are permitted in our **Design System Applied** approach.
* **Every Design token must be present in the design-tokens stylesheet** — both required and optional.
* **The front-matter path → CSS variable mapping must follow the tables** in sections 2–7. The "Token path" column is the front-matter path; the "CSS variable" column is the stylesheet variable. A token with no `Token path` is a **stylesheet-only** token (font sizes, weights, line heights, letter spacing): it is defined only in the stylesheet and is not declared in the front matter.
* **Non-required token defaults must be explicit.** They are either a `var()` reference to a required token or a documented literal value. The "Default" column for every optional token in the tables specifies its fallback, and the stylesheet must use that exact fallback when the token is absent from the front matter.
* **The "drop `.base`" rule** — when a token path ends in `.base`, the CSS variable drops the `base` segment (e.g. `colors.text.base` → `--color-text`, not `--color-text-base`).
* **Derived color variants (`muted` / `active`) are NOT tokens.** They must not appear in the front matter. They are generated in `color-variants.css` from the base brand and action colors via HSL relative color syntax. The `DESIGN.md` `## Colors` section documents the derivation rule in prose only.
* **`border` holds border widths only.** Border *colors* are not tokens — components pick the color tokens they need for their border variants under the `components:` key.
* **Merged paths in YAML** — when several token paths share the same parent object (e.g. `typography.base.fontFamily` and `typography.base.lineHeight`), they collapse into a single `typography.base` entry in the front matter. See section 8.
* **Hosted web fonts are imported.** `design-tokens.css` must load every hosted web font (for example a Google Font) that a `--font-family-*` value names. Use an `@import` above every rule, or a matching `@font-face`. System-only stacks (`ui-serif`, `Georgia`, …) need none. Re-check this rule after a font-family edit.
* **Resolved color contrast pairs are present and correct.** Every `--color-on-*` token from section 3 must appear in `design-tokens.css`, and its value must equal what the derivation rule computes from the theme's own colors. A missing or stale pair is a contract error. Re-check this rule after any color edit.
* **The base text/surface pair meets AA.** `--color-text` must contrast at least 4.5:1 against `--color-surface`. `--color-surface` has no `--color-on-*` companion; `--color-text` is its foreground by rule. Re-check this rule after editing either color.
