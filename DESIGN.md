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
        primary: "#ea5514" # --color-brand-primary   (record button, waveform, highlights)
        accent: "#ff8a3d" # --color-brand-accent    (live text, active glow)
        secondary: "#2a2a32" # --color-brand-secondary (raised chrome, inactive bars)
        # tertiary: omitted → --color-brand-tertiary: var(--color-brand-primary)

    # Action — all four are required
    action:
        success: "#35c48b" # --color-action-success  (saved, connected)
        info: "#4d9dff" # --color-action-info     (neutral notices)
        warning: "#f0a82e" # --color-action-warning  (paused state)
        danger: "#e5484d" # --color-action-danger   (errors, stop)

    # Text  (drop-.base rule: colors.text.base -> --color-text)
    text:
        base: "#f2efeb" # --color-text
        accent: "#ff8a3d" # --color-text-accent (streaming / highlighted words)
        muted: "#9c9aa3" # --color-text-muted
        ondark: "#f2efeb" # --color-text-ondark

    # Surface  (drop-.base rule: colors.surface.base -> --color-surface)
    surface:
        base: "#17171b" # --color-surface      (floating panel body)
        alt: "#202027" # --color-surface-alt  (inset transcript well, inputs)
        dark: "#0e0e11" # --color-surface-dark (top info strip, modal backdrop)
        card: "#1c1c22" # --color-surface-card (settings card)

# ============================================================================
# TYPOGRAPHY  —  entries are objects;
# fontFamily, fontWeight, lineHeight, letterSpacing values MUST use existing
# preset values from design-tokens.css (CSS variables)
# ============================================================================
typography:
    base:
        {
            fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Helvetica Neue", Arial, sans-serif',
            fontWeight: "var(--font-weight-regular)",
            lineHeight: "var(--line-height-regular)"
        }
    display:
        {
            fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Helvetica Neue", Arial, sans-serif',
            fontWeight: "var(--font-weight-extrabold)",
            lineHeight: "var(--line-height-tight)",
            letterSpacing: "var(--letter-spacing-tight)"
        }
    mono:
        {
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
    sm: "0.375rem"
    md: "0.75rem"
    lg: "1.25rem"
    full: "100%"

# ============================================================================
# SPACING  —  spatial rhythm scale, 4px linear (all required)
# ============================================================================
spacing:
    xs: "0.25rem"
    sm: "0.5rem"
    md: "0.75rem"
    base: "1rem"
    lg: "1.25rem"
    xl: "2rem"
    xxl: "3rem"

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

The widget has five screens — the **mode chooser**, the **setup form**, the
**take screen**, the **file transcription** screen and the **session end**
result — plus the floating **Settings** and **Help** dialogs. The take screen
has three take states: **idle**, **recording** and **paused**. Recording and
Paused must be distinguishable from across the room, so status is carried by
color, label, dot, and waveform shape all at once — never by color alone.

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
- **Warning (`#f0a82e`)** — The **Paused** state: status dot and label. Amber
  is deliberately far from the recording orange so the two never blur
  together; the record control returns to its dark face at the same time.
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
`1.5625rem`, and `2xl` `1.953rem`.

**The application uses three of those steps, and only three.** Every string is
rendered by one of three components, so a role always looks the same on every
screen:

| Component | Role                     | Token            | Size    | Weight   |
| --------- | ------------------------ | ---------------- | ------- | -------- |
| `Title`   | screen and card titles   | `--font-size-lg` | 20 px   | semibold |
| `Body`    | body copy, settings rows | `--font-size-md` | 16 px   | regular  |
| `Label`   | captions, badges, hints  | `--font-size-sm` | 12.8 px | medium   |

A view never passes a size or a weight: it picks the role, and may add only
`.muted()`, `.tone(..)`, `.mono()`, `.bold()` (the single emphasis step above
the role's weight) or `.center()`. `--font-size-xs`, `--font-size-xl` and
`--font-size-2xl` stay in the token file as the stylesheet's record but are not
used by the application; `crates/speechtt-ui/tests/design_system.rs` fails the
build if a `text_size` or `font_weight` appears outside
`components/text.rs`. Shortcut hints and read-outs use `.mono()` on one of the
three roles, not a separate size.

## Layout

A 4px linear spacing scale with `1rem` (16px) as the base step. The floating
panel opens at 600 x 460 px (or the size and position remembered from the last
run) with `--rounded-lg` corners and `--space-base` internal padding. It can
never be resized below 600 x 378 px: that is the smallest box that fits the
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

### Application header

Rendered by `components::AppHeader` in the widget window and in every dialog
window. One bold `Body` title on the left over the `title_bar` role, a 1px
`title_bar_border` hairline below, and `IconButton` actions on the right in
insertion order. A caller may attach a close button or another window action
without changing the strip.

The widget window appends its compiled version after the title through
`AppHeader::version`; dialog windows do not, because the Help footer already
carries the version and its header would otherwise say it twice in one window.
The version is a `Label` with `.mono().muted()` — `--font-size-sm`,
`--font-family-mono`, `--color-text-muted` — so it reads exactly like the
`SPEECHTT V<version>` line in the Help footer. It shows `V<major>.<minor>.<patch>`,
the value compiled into the binary, never a literal in the view.

### Round button

Rendered by `components::RoundButton` — never a raw `div` sized at the call
site. Exactly one diameter (56 px, `rounded.full`), two tones:

- `Primary`: `brand.primary` fill with a `brand.primary` outline, hover
  `brand.primary-active` with a `brand.primary` outline.
- `Secondary`: `surface` fill with a 1px `brand.secondary` outline, hover
  `surface-alt-active` with a `brand.primary` outline.

Both tones use the same ink for their glyph (`--color-text`): a tone never
tints the icon. The 2px outline is always present and only changes colour —
tone colour at rest, `brand.primary` on hover — so hovering never nudges the
layout; the fill switches to the tone's active variant at the same time. There
is no separate hover-only style for the card case: the round button always
shows the hover and press treatment, which is what makes the whole card read
as one control.

### Press wave

A press spreads one filled circle from the button. It is the **button's own
colour**, starts at the **button's own diameter and centre**, and grows by
`RIPPLE_TRAVEL` (20 px) while its opacity falls from 1 to 0 over exactly that
travel — so at rest it is completely hidden behind the button face and all that
is ever visible is a thin slice of the disc appearing at the edge and vanishing.
It runs for 450 ms and never affects layout.

Rendered by `components::RippleRing` at the **window root**, above the screen.
Being the button's colour, the part that overlaps the button is invisible, which
is what makes the wave read as emerging from behind the button rather than from
on top of it. The window owns it — not the pressed screen — because every round
action navigates (chooser → form, the form → the take screen, the take
screen → the chooser); a wave drawn inside the pressed screen would be
destroyed on release. Its centre comes
from the anchor the button records while prepainting
(`components::button_anchors`), so the wave is concentric with the button no
matter where inside it the pointer landed.

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
mode** at `Title` (lg) semibold, description **Select a transcription method**
at `Body` (md) muted, then two equal cards (live dictation, from a file). The
title and description are left-aligned on the same left spine as the cards'
left edge; free window space grows on the right, never inside the cards. Both
cards are pinned to `MODE_CARD_WIDTH` (272 px) and sit `--space-base` apart.
Each card centres a `Title` and a muted `Body` description, then pushes a
`RoundButton` with its `Label` caption (`Start`, `Browse files`)
`--space-sm` beneath it to the card bottom. Because the button block is pinned
to the bottom, both buttons share one baseline however the descriptions wrap.

### Session setup form

Two `SettingRow`s: **Transcription Service** and **Model**, each owning a small
select. The provider select is a non-searchable `components::ProviderSelect`
menu listing every provider from the capability catalogue; choosing a provider
commits it and clears the dependent model. The model select is a searchable
`components::ModelSelect` menu whose rows show the model label followed by its
`TranscriptionModeBadge`; the same badge stays beside the field for the current
draft.

The form opens already seeded from the Settings defaults: a live take starts from
**Default Streaming Model** and a file take from **Default Batch Model**, so the
two fields are filled and Continue is enabled without a visit to the menus.

In **file** mode the body is preceded by an `InfoNote` reading _"Files are
transcribed in batches, so only non-streaming models are listed."_, because a
file take cannot use a streaming model; the model menu then lists only
**Batch** entries. The note uses the existing component as-is: no new tone, size
or spacing. Both menus take their rows from the handshake catalogue, so no
provider or model is named in the view.

### Toggle round button

`components::ToggleRoundButton` is the `RoundButton` as a **switch**: one glyph,
two states, on or off. The caller passes the icon and the state; the two tones
below and the motion that marks "on" belong to the component.

The take screen uses it for its record button, at the same 56 px diameter as
every round button. That button speaks the language of **capture**, not of
playback — the glyph is always the microphone, never a play, pause or square:

| Take state | Control | Face                                                                                   | Motion                 |
| ---------- | ------- | -------------------------------------------------------------------------------------- | ---------------------- |
| idle       | **off** | `surface` fill, `brand.secondary` outline (`RoundTone::Secondary`), `--color-text` mic | none                   |
| recording  | **on**  | `brand.primary` fill and outline (`RoundTone::Primary`), `--color-text` mic            | pulse + repeating wave |
| paused     | **off** | as idle                                                                                | none                   |

The control uses the two tones §Round button defines and introduces no colour of
its own; "off" is the same resting dark face as the chooser's file card. "Inert"
is visual only — the off control still accepts a click, because pressing it is
how a take starts and how a paused take resumes.

While on, two animations run for the same `TOGGLE_PULSE_PERIOD` (1.6 s),
both in the button's own colour and both drawn **behind** the face, so only the
sliver past the button edge is ever visible:

- a **pulse**: the resting disc drawn at `pulse_scale`, a 6 % swell
  (1.00 → 1.06 → 1.00) eased with a raised cosine, so the loop has no seam;
- a **ring wave**: one per period, from the button diameter to
  `TOGGLE_WAVE_MAX_SIZE`, fading 1 → 0 — the same growth-and-fade as the press
  wave.

Both start when the switch turns on and stop when it turns off, so a paused take
is completely still. The press wave still fires on every click, unchanged.

The take screen's footer puts this control in the centre zone:

| Zone     | Action                    | Behaviour                                                                                    |
| -------- | ------------------------- | -------------------------------------------------------------------------------------------- |
| Leading  | `ArrowBigLeft`, "Cancel"  | discards an open take, stops capture, returns to the mode chooser                            |
| Primary  | `ToggleRoundButton` (mic) | off → starts or resumes; on → pauses                                                         |
| Trailing | `RotateCcw`, "Reset"      | clears the take and stays on the screen; dimmed when idle or for a file take                 |
| Trailing | `ArrowBigRight`, "Stop"   | ends the take and shows the session end screen; live whenever a take is open, no pause first |

The trailing Stop is a **forward arrow**, the mirror of the footer's back arrow:
ending the take is going forward, to the transcript. No playback glyph (play,
pause, square) appears on this screen; the record control is always the
microphone.

Idle, recording and paused are three states of this one screen: the status pill
and the waveform say which, and colour is never the only signal.

The header's trailing edge holds **two** things, left to right: the model, then
the elapsed `mm:ss` timer. The clock is attached in every state, including before
the microphone is pressed — `00:00` on an idle screen is what makes the later
`00:07` legible. A file take shows the elapsed read-out alone; its model was
chosen on the setup form and never changes.

The model is a **picker** while the session can still change it, and plain text
once it cannot:

| Session state                             | Trailing model                                         | Why                                                                                     |
| ----------------------------------------- | ------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `Ready` — before the first recorded frame | `LiveModelSelect` dropdown, opened on the take's model | the user may still change engines without leaving the screen                            |
| `Recording` / `Paused`                    | muted `Label` with the model name                      | the machine freezes its configuration from the first frame, so a control would be a lie |
| no catalogue, or no model resolves        | nothing                                                | the clock still shows                                                                   |

The picker is **take-scoped**: it changes the model of the take about to be
recorded and never rewrites the Settings default. A live take may use a
_segmented_ model — which batches the microphone audio — and `model_streaming`
accepts only a streaming one, so writing the choice back would break the default
seeding the Settings panel guarantees.

The streaming/batched distinction belongs to the model picker, where a model is
chosen: a model row in these menus carries its `TranscriptionModeBadge`, so the
header names the engine without repeating the chip.

### File transcription

A file take has no microphone and no live transcript, so it gets its own screen
instead of the take screen. One quiet report, and three actions:

| Band   | Content                                                                                                                                                                                                                                                                                                                                  |
| ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Header | `In progress` pill — `action.warning` dot, muted label — while the worker decodes; `Done` pill — `action.success` dot and label — once it has finished; `STOPPED` pill — `action.danger` dot and label — when the user ended the decode early. Trailing: the elapsed `mm:ss` read-out, in the same place and style as on the take screen |
| Body   | One `SurfaceBox` with the transcribed file's name (`Label`, mono) and a `ProgressBar`                                                                                                                                                                                                                                                    |
| Footer | Back (left) → mode chooser · Stop (centre) → end the decode early · Continue (right) → the session end screen                                                                                                                                                                                                                            |

The corner read-out times the **take**, not the file: the clock starts when the
transcription starts and stops when the worker finishes, exactly like a live
take. The position inside the file is the progress row's business.

`components::ProgressBar` reports that position with the same vocabulary as the
live waveform: a dense sine wave of 80 rounded bars tracing ten complete
oscillations, each period rising from a trough to a peak and back, in the same
bar band the waveform uses. The row is revealed from the left as the worker
advances. Revealed bars are
`brand.primary`, the remainder stays on the theme's border colour, so the full
length is always visible and the reveal reads as a fill. Below the row sits a
`Label` read-out of the decoded position: `12 / 60 s` when the worker reported the
duration, `12 s` when it did not. An unknown duration leaves the row unrevealed
and only the read-out advances.

The screen never navigates on its own. When the file ends, Stop becomes inert
and Continue enables; the user decides when to open the transcript. Stop pressed
mid-decode keeps the text produced so far and lands in the same state, with the
pill reading `STOPPED` so an interrupted transcription never reads as a
completed one. Back discards the take from either state.

The session end screen carries **no status pill**: the take is over, and what is
left to do is copy or save.

### Waveform

32 rounded bars, 3px wide, `rounded.full`, gap `--space-xs`. Bar height maps
to the FFT magnitude of its frequency band, from `--space-xs` (silence) to
`--space-xl` (peak). Recording bars are `brand.primary` with the newest few at
`brand.accent`. Paused bars drop to `brand.secondary` at a flat, low height so
the panel visibly exhales.

### Transcript well

Inset `surface-alt` panel, `rounded-md`, `space-md` padding, `elevation-sm`.
Body text in `text` at `--font-size-sm`, line-height `regular`. Arriving words
are briefly `text.accent`, settling to `text` after ~600ms. A soft fade at the
top edge hints at earlier text.

The transcript scrolls vertically inside the panel, and the panel shows no
scrollbar: the wheel and the keyboard reach every earlier sentence. The panel
keeps the height the screen gives it, so a long take is never truncated — the
full transcript stays in the buffer for copy and save. During a live take the
well follows the newest line while the reader is at the bottom; scrolling up
stops the follow, and scrolling back to the bottom resumes it. The result
screen opens at the start of the transcript and does not follow.

The transcript is the application's one selectable text. A drag across it
selects the range under the pointer, from a single word to several sentences,
and the platform copy shortcut puts that range on the clipboard: `Ctrl-C` on
Linux, `Cmd-C` on macOS. Paste happens in whatever application the user pastes
into — the widget gains no clipboard-reading path of its own.

Copying is an application action: the widget window binds the platform copy key
under its own key context, reads the highlighted range from the window selection
and writes it to the clipboard. The copied string is exactly the highlighted
range, and an empty or whitespace-only selection leaves the clipboard untouched.

A right-click inside the transcript opens a menu with a single `Copy` entry,
disabled while nothing is selected. It carries the same action as the shortcut,
so the entry shows that shortcut's keybinding and the two routes can never
diverge. One entry and no more: the widget has no paste path, and the transcript
is not an editable field, so neither `Paste` nor `Select All` belongs here. The
menu is the library's own component — it takes its surface, colours, radii,
spacing and elevation from the theme, and the transcript adds no colour, spacing
or elevation of its own.

The selected range is washed with `brand.primary` at 30 % opacity, painted
behind the glyphs. One role with an alpha rather than a new colour: it stays
readable under both the muted ink of the committed text and the `brand.accent`
of the live tail, so a selection that spans the two never hides the boundary it
crossed. A drag that crosses the boundary copies the committed text first, then
the live words, as one document read top to bottom.

Nothing else is selectable. Titles, labels, badges, notes, buttons and dialog
copy stay inert, so a drag that starts on a button is a click, not a selection.

### Status pill

Small pill (`rounded.full`) at the top-left of the transcript section: a 8px
dot plus an uppercase `mono` label at `--font-size-xs`,
`--letter-spacing-wide`. A resting state colours dot and label alike; a working
state (recording, decoding) keeps its label muted beside a coloured dot, so the
pill reads as one signal without shouting.

| State                               | Dot                      | Label            |
| ----------------------------------- | ------------------------ | ---------------- |
| idle (take screen, ready to record) | `action.success` (mint)  | `action.success` |
| recording                           | `action.danger` (red)    | `text.muted`     |
| paused                              | `action.warning` (amber) | `action.warning` |
| decoding a file                     | `action.warning` (amber) | `text.muted`     |
| file decoded                        | `action.success` (mint)  | `action.success` |
| file take ended early               | `action.danger` (red)    | `action.danger`  |

Green says "the microphone is free, press it" or "the file is transcribed"; red
is capture, amber is a held or working state. No colour is invented for this: the
mint is the existing `--color-action-success` role that also means _saved_ and
_connected_. Colour is never the only signal — the label spells the state out.

### Action buttons

Rendered by `components::ActionButton`, never a raw `Button` with a size set
at the call site. One medium size everywhere: `--font-size-md`, 32px high,
`--space-sm`/`--space-md` horizontal padding. `Primary` marks the single
committing action in a group (Save, Continue, Got it); `Secondary` marks the
dismissing or alternative action (Back, Cancel). No other tones are needed.

### Screen footer

Every widget screen with actions renders the same footer, built by
`components::ScreenFooter`. It is one strip at the bottom edge of the window —
`title_bar`, a 1px `title_bar_border` hairline on top, `--space-lg` horizontal
padding — split into three zones:

| Zone     | Meaning                                                    |
| -------- | ---------------------------------------------------------- |
| Leading  | **Back**, one step towards the mode chooser                |
| Primary  | The screen's main action, centred on the footer's own axis |
| Trailing | Secondary actions, left to right in insertion order        |

The side zones are equal flexible columns, so the primary action stays centred
whether a side is empty or full. Back is an `IconAction` (34 px, the same
square as every other secondary action) carrying `ArrowBigLeft`, so the 56 px
round button is unmistakably the main one. A screen with no actions renders no
footer at all.

A step inside a flow pairs its exits as two arrows: back on the left and
forward on the right, the same square, the same glyph family, opposite
directions. That pairing says "you are between two steps" without a label, and
forward is dimmed until the step is complete.

| View                     | Leading                                                    | Primary      | Trailing                                                                          |
| ------------------------ | ---------------------------------------------------------- | ------------ | --------------------------------------------------------------------------------- |
| `welcome` (mode chooser) | —                                                          | —            | — (no footer)                                                                     |
| `session_setup`          | Back → mode chooser (drops the draft and any pending file) | —            | Forward (`ArrowBigRight`, disabled until the draft is complete) → starts the take |
| `recording` (live take)  | Back → mode chooser                                        | take control | Stop                                                                              |
| `session_end`            | Back → mode chooser                                        | —            | Clean up + Copy + Save, or Open file                                              |
| `session_transcription`  | Back → mode chooser                                        | Stop         | Continue                                                                          |

Back means the same thing everywhere: one step towards the mode chooser, with
no stale draft, pending file, finished take, output status or error strip left
behind.

**Trailing actions read left to right in the order the user is expected to
reach them.** On the finished take that order is: the cleanup toggle, Copy,
Save.

The cleanup is **one** button, not two. Its glyph and label name the action it
will take next, so a single slot carries the whole gesture:

| Screen shows                       | Icon           | Label                | Effect                        |
| ---------------------------------- | -------------- | -------------------- | ----------------------------- |
| no repair yet                      | `WandSparkles` | "Clean up text"      | runs the pass                 |
| a repair running                   | `WandSparkles` | "Cleaning up…"       | disabled                      |
| the repaired text                  | `RotateCcw`    | "Show original text" | shows the pre-repair text     |
| the original, with a repair cached | `WandSparkles` | "Show cleaned text"  | shows the repaired text again |

| Action         | Icon                         | Label     | Enabled when                                                                         |
| -------------- | ---------------------------- | --------- | ------------------------------------------------------------------------------------ |
| Cleanup toggle | `WandSparkles` / `RotateCcw` | see above | the take has text, and either a cleanup model resolves or a repair is already cached |
| Copy           | `Copy`                       | "Copy"    | the take has text                                                                    |
| Save           | `HardDriveDownload`          | "Save"    | the take has text                                                                    |

The toggle is a secondary `IconAction` like Copy and Save — same 34 px square,
same `secondary` surface. It carries no accent colour: the repair it starts is
not the screen's main action, and the filled round button keeps that role. While
the request is outstanding the toggle is dimmed and the muted status line under
the transcript reads "Cleaning up…"; once the answer lands it reads "Cleaned",
and a flip back to the original reads "Original restored".

The pass runs **once per take**. Both halves — the original and the repaired
text — stay cached for the life of the take, so flipping between them is local
and instantaneous and never calls the model again. The glyph therefore names the
action rather than the state: the wand means "end up on the repaired text", the
reverse arrow "go back to the original". A cached repair stays flippable even if
the cleanup model stops resolving, because the user's two texts are already in
hand.

A written take replaces the whole trailing zone with Open file: the text is
already on disk, so there is nothing left to repair or copy.

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
separated by 1px `brand.secondary` hairlines. The rows read top to bottom:

1. **Bun path** — the runtime the worker needs.
2. **API key** — the credential, as a stacked row: status badge on the label
   line, masked field spanning the row below.
3. **Default Streaming Model**
4. **Default Batch Model**
5. **Cleanup Model**
6. **Dictation Language**

**Transcription Service** sits between the API key and the take defaults only
while the catalogue offers more than one provider. A catalogue with at most one
provider puts that provider in the draft (`DEFAULT_PROVIDER` before any
catalogue arrives, and the catalogue's own id once it does), so the absence of
the row means there is nothing to choose: Save already has a provider. The row
appears by itself as soon as a second provider exists.
Labels in `mono` uppercase `--font-size-xs`; values in `base` `--font-size-sm`.
A description that is itself an identifier — an environment-variable name such
as `AI_GATEWAY_API_KEY`, a path — is drawn in `mono` instead, because it reads
as code rather than prose (`SettingRow::mono_description`).

Rows use the dense symmetric rhythm: one step tighter than the roomier setup
form, with equal vertical padding above and below the content, so a divider sits
centred between two rows. Consecutive rows are stacked without a gap; notes stay
separate children of the dialog body and keep its gap.

Every select uses the same components as the Session setup form, against the
settings draft: `components::ProviderSelect`, `components::ModelSelect` and
`components::LanguageSelect`. The provider menu lists every catalogue provider,
and its empty trigger reads `Choose provider`. Both default-model menus stay
disabled until a catalogue has arrived: a pinned provider names no model on its
own, and a menu with nothing in it must not open. Each default-model row is
dedicated to one take mode, so the row carries no `TranscriptionModeBadge`
beside its trigger; the badge stays inside the menu, where one list can mix both
modes.

Two states leave the dialog nothing to save, and each says so in an `InfoNote`
under the rows: a catalogue that reports no provider at all names that fact, and
a backend that never started names its cause — for a missing runtime, the paths
the lookup searched. A note disappears with the state it describes.

The API-key badge reports `NOT VERIFIED` until a catalogue has arrived, because
availability is what the badge claims. With a catalogue it reads `CONNECTED`
when a key is stored and the provider is not reported unavailable, and
`NOT CONNECTED` otherwise.

**Bun path** — a `SettingRow` with an `Input`, on the first row because the
worker cannot start without the runtime. Empty is the normal state: the runtime
is resolved from the configured path, then `PATH`, then `~/.bun/bin/bun`,
`/usr/local/bin/bun` and `/opt/bun/bin/bun`. A value is the user's explicit
override, needed when a desktop launcher's `PATH` does not carry the directory a
shell profile added. It is optional and never blocks Save.

**Dictation Language** — _"The language you speak; improves cleanup accuracy"_;
it closes the list because it fine-tunes the cleanup pass rather than deciding
the transcription. Its first entry is **Auto (detect)**, whose value is the
empty language; every other entry is an ISO-639-1 code the worker published, in
the worker's order. An unset language therefore renders as Auto, never as a
blank field. The menu is not searchable: one entry is a single word.

The two model rows are one default per take mode, each a `SettingRow` on its own
line because two dropdowns do not fit side by side:

- **Default Streaming Model** — _"Used for live dictation"_; lists only
  `Streaming` models.
- **Default Batch Model** — _"Used for audio and video files"_; lists only
  `Segmented` models.

Both are required to save, so every take starts from a model of its own mode. The
provider menu lists every catalogue provider; an arriving catalogue refreshes the
open dialog.

**Cleanup Model** — _"Repairs typos and broken sentences after transcription"_,
below the two model rows because it never transcribes: it repairs a finished
transcript. It lists the text models the worker discovered, with the placeholder
"Use DEFAULT_LANGUAGE_MODEL" while nothing is stored. A worker that reported no
language models drops the menu and names the resolved default id as a `mono`
value instead, so the row still reads as configured rather than as an empty
field.

The dictation language and the cleanup model are both **optional**: neither
blocks Save, a take starts without either, and the cleanup action stays disabled
until a model resolves.

## DO's and DON'Ts

- Do let the filled primary round button be the only saturated shape on a
  screen.
- Do signal Recording vs Paused with dot + label + waveform shape, not color alone.
- Do use `--color-on-*` for anything drawn on a brand or action background.
- Do keep shortcut reminders visible in the top strip while recording.
- Do not use `text.accent` for body copy — it is for newly arrived words only.
- Do not use the recording orange for the paused state; amber owns pause.
- Do not introduce ad-hoc colors, radii, or shadows outside this token set.
