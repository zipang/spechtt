//! SpeechTT token constants — the Rust mirror of `design-tokens.css`.
//!
//! Single source of truth for visual values in `speechtt-ui`. Components
//! must read these (or the mapped theme roles in [`crate::theme`]) instead of
//! writing raw values; `tests/theme_contract.rs` guards the parity between
//! this module and the stylesheet.
//!
//! The pixel values below resolve `design-tokens.css` rem values at the
//! 16px base font (`--font-size-base: 1rem`). They live in this module only —
//! this is the theme/token definition the coding guides allow — while all
//! consuming components reference the constants.

/// `--color-*` token values as 0xRRGGBB hex, keyed by the CSS variable name
/// suffix. Parse into [`gpui::Hsla`] with `rgb(const)`.
pub mod color {
    /// `--color-brand-primary` — deep Agfa-Sensor orange.
    pub const BRAND_PRIMARY: u32 = 0xea5514;
    /// `--color-brand-accent` — bright orange; hover lift, live text.
    pub const BRAND_ACCENT: u32 = 0xff8a3d;
    /// `--color-brand-secondary` — slate charcoal hairlines/borders.
    pub const BRAND_SECONDARY: u32 = 0x2a2a32;
    /// `--color-surface` — near-black panel body.
    pub const SURFACE: u32 = 0x17171b;
    /// `--color-surface-alt` — raised wells, inputs, chips.
    pub const SURFACE_ALT: u32 = 0x202027;
    /// `--color-surface-dark` — top info strip, dialog backdrop.
    pub const SURFACE_DARK: u32 = 0x0e0e11;
    /// `--color-surface-card` — settings/dialog card surfaces.
    pub const SURFACE_CARD: u32 = 0x1c1c22;
    /// `--color-action-success` — mint; saved, connected.
    pub const SUCCESS: u32 = 0x35c48b;
    /// `--color-action-info` — blue; neutral notices.
    pub const INFO: u32 = 0x4d9dff;
    /// `--color-action-warning` — amber; PAUSED state.
    pub const WARNING: u32 = 0xf0a82e;
    /// `--color-action-danger` — red; errors.
    pub const DANGER: u32 = 0xe5484d;
    /// `--color-text` — warm off-white body text.
    pub const TEXT: u32 = 0xf2efeb;
    /// `--color-text-muted` — hints, timestamps, status hints.
    pub const TEXT_MUTED: u32 = 0x9c9aa3;
    /// `--color-on-primary` (and every `--color-on-*` colored fill) — black ink.
    pub const ON_COLOR: u32 = 0x000000;
}

/// Corner radii in px: `--rounded-sm/md/lg` resolved at 16px base.
pub mod radius {
    /// Keycaps, chips, inputs (0.375rem).
    pub const SM: f32 = 6.0;
    /// Buttons, wells (0.75rem).
    pub const MD: f32 = 12.0;
    /// Floating panel, dialog surface (1.25rem).
    pub const LG: f32 = 20.0;
}

/// Spacing scale in px: `--space-xs..xxl` on the 4px grid.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const BASE: f32 = 16.0;
    pub const LG: f32 = 20.0;
    pub const XL: f32 = 32.0;
    pub const XXL: f32 = 48.0;
}

/// Typographic scale in px: `--font-size-*` resolved at the 16px base
/// (geometric ×1.25 ratio). `MD` == 1rem == the theme base font size.
pub mod font {
    /// `--font-size-xs` (0.64rem) — keycaps, status labels, hints.
    pub const XS: f32 = 10.24;
    /// `--font-size-sm` (0.8rem) — secondary text, transcript.
    pub const SM: f32 = 12.8;
    /// `--font-size-md` (1rem) — body text; the rem anchor.
    pub const MD: f32 = 16.0;
    /// `--font-size-lg` (1.25rem) — titles, settings labels.
    pub const LG: f32 = 20.0;
    /// `--font-size-xl` (1.5625rem) — panel titles.
    pub const XL: f32 = 25.0;
    /// `--font-size-2xl` (1.953rem) — hero numerals.
    pub const TWO_XL: f32 = 31.25;
}

/// The same typographic scale expressed in `rem`, for builders that resolve
/// against the window's rem size (`rems(rem::SM)`) so type follows theme zoom.
pub mod rem {
    pub const XS: f32 = 0.64;
    pub const SM: f32 = 0.8;
    pub const MD: f32 = 1.0;
    pub const LG: f32 = 1.25;
    pub const XL: f32 = 1.5625;
    pub const TWO_XL: f32 = 1.953125;
}

/// Window dimensions of the widget.
///
/// Wide enough for the mode chooser's two [`MODE_CARD_WIDTH`] cards side by
/// side, the gap between them and the body inset (and for the provider/model
/// form); tall enough for the two prominent rows plus the optional error
/// strip. A remembered size overrides it on later launches, and the body
/// scrolls if a future row is added.
pub const WIDGET_WIDTH: f32 = 600.0;
pub const WIDGET_HEIGHT: f32 = 460.0;

/// Smallest window the widget accepts.
///
/// The mode chooser needs it: its two cards, the gap between them and the body
/// inset must never be squeezed, or the cards would overlap the welcome title.
/// With the mockup's 272 px cards the minimum equals the default width, so the
/// window can no longer be narrowed below 600 px. The window manager enforces
/// it, and a remembered smaller geometry is raised to it on launch.
pub const MIN_WINDOW_WIDTH: f32 = 600.0;
pub const MIN_WINDOW_HEIGHT: f32 = 360.0;

/// Width of one mode-chooser card.
///
/// The chooser keeps its cards at this intrinsic size and centers the pair, so
/// extra window space grows around the cards instead of inside them. Matches
/// the mockup (`mockups/app-window/new-session.png`): both descriptions stay
/// on two lines and the buttons align.
pub const MODE_CARD_WIDTH: f32 = 272.0;
