//! SpeechTT theme — `design-tokens.css` mapped onto the gpui-kit
//! `gpui-component` Theme.
//!
//! Mapping contract (see `PRODUCT.md` Phase 2):
//!
//! | CSS token                      | Theme role                    |
//! |--------------------------------|-------------------------------|
//! | `--color-surface`              | `background`                  |
//! | `--color-text`                 | `foreground`                  |
//! | `--color-text-muted`           | `muted_foreground`            |
//! | `--color-surface-dark`         | `title_bar`, `title_bar_border` bg |
//! | `--color-surface-card`         | `popover` (+ popover fg)      |
//! | `--color-surface-alt`          | `secondary`, `input`, `muted`, `button` |
//! | `--color-brand-secondary`      | `border`, hover states        |
//! | `--color-brand-accent`         | `ring`, primary hover         |
//! | `--color-brand-primary`        | `primary`                     |
//! | `--color-action-*`             | `success` / `info` / `warning` / `danger` |
//! | `--color-on-*`                 | every `*_foreground` on a colored fill |
//!
//! `--rounded-sm` (6px) → `theme.radius`, `--rounded-lg` (20px) →
//! `theme.radius_lg`; `--font-size-md` (16px) → `theme.font_size` (the rem
//! anchor), `--font-size-sm` (12.8px) → `theme.mono_font_size`.
//!
//! Tokens with no ThemeColor counterpart (`--rounded-md` 12px, elevation
//! shadows, the geometric type scale steps) stay as constants in
//! [`crate::tokens`] and are consumed directly by SpeechTT components —
//! per the coding guides, application-owned presentation state.
//!
//! Typography is bundled: Inter + JetBrains Mono are compiled into the
//! binary and registered via the text system, pinning the rendering
//! across machines (see [`FONT_FAMILY_SANS`] / [`FONT_FAMILY_MONO`]).
//!
//! `tests/theme_contract.rs` guards the parity with the stylesheet.

use crate::tokens::{color, font, radius};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, px, rgb};
use std::{borrow::Cow, sync::Once};

/// Bundled typefaces (crates/speechtt-ui/assets/fonts/, SIL-OFL licenses
/// included alongside the `.ttf` files). Embedding gives SpeechTT the same
/// rendering on every machine — metrics, weight fallbacks and the caption
/// well's mono layout are deterministic — at the cost of ~1.6 MB binary
/// size. The `design-tokens.css` `--font-family-*` system stacks are the
/// web-mockup record; the native layer pins its own families here.
const BUNDLED_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
];

/// Sans family — `--font-family-base`'s native equivalent.
pub const FONT_FAMILY_SANS: &str = "Inter";

/// Mono family — `--font-family-mono`'s native equivalent (used for the
/// live transcript / caption well).
pub const FONT_FAMILY_MONO: &str = "JetBrains Mono";

/// `add_fonts` must run once per process; registering the same files
/// twice is at best wasted work.
fn embed_fonts(cx: &mut App) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        cx.text_system()
            .add_fonts(BUNDLED_FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect())
            .expect("bundled fonts must load (broken build asset?)");
    });
}

/// Installs the SpeechTT Sensor dark theme as the application's active
/// theme. MUST be called after `gpui_kit::init(cx)` and before opening
/// windows, so every component renders with SpeechTT colors from the
/// first frame.
pub fn apply(cx: &mut App) {
    // Bundled typefaces first, so any layout GPUI performs during theme
    // activation can already resolve `Inter` / `JetBrains Mono`.
    embed_fonts(cx);

    Theme::change(ThemeMode::Dark, None, cx);

    Theme::update(cx, |theme| {
        let c = &mut theme.colors;

        // Surfaces
        c.background = rgb(color::SURFACE).into();
        c.foreground = rgb(color::TEXT).into();
        c.title_bar = rgb(color::SURFACE_DARK).into();
        c.title_bar_border = rgb(color::BRAND_SECONDARY).into();
        c.popover = rgb(color::SURFACE_CARD).into();
        c.popover_foreground = rgb(color::TEXT).into();
        c.secondary = rgb(color::SURFACE_ALT).into();
        c.secondary_foreground = rgb(color::TEXT).into();
        c.secondary_hover = rgb(color::BRAND_SECONDARY).into();
        c.secondary_active = rgb(color::BRAND_SECONDARY).into();
        c.input = rgb(color::SURFACE_ALT).into();
        c.muted = rgb(color::SURFACE_ALT).into();
        c.muted_foreground = rgb(color::TEXT_MUTED).into();

        // Chrome
        c.border = rgb(color::BRAND_SECONDARY).into();
        c.ring = rgb(color::BRAND_ACCENT).into();

        // Primary brand
        c.primary = rgb(color::BRAND_PRIMARY).into();
        c.primary_foreground = rgb(color::ON_COLOR).into();
        c.primary_hover = rgb(color::BRAND_ACCENT).into();
        c.primary_active = rgb(color::BRAND_ACCENT).into();

        // Neutral buttons (wells)
        c.button = rgb(color::SURFACE_ALT).into();
        c.button_foreground = rgb(color::TEXT).into();
        c.button_hover = rgb(color::BRAND_SECONDARY).into();
        c.button_active = rgb(color::BRAND_SECONDARY).into();

        // Action colors — all fills carry black ink (`--color-on-*`).
        c.success = rgb(color::SUCCESS).into();
        c.success_foreground = rgb(color::ON_COLOR).into();
        c.info = rgb(color::INFO).into();
        c.info_foreground = rgb(color::ON_COLOR).into();
        c.warning = rgb(color::WARNING).into();
        c.warning_foreground = rgb(color::ON_COLOR).into();
        c.danger = rgb(color::DANGER).into();
        c.danger_foreground = rgb(color::ON_COLOR).into();

        // Typography — the base font is the rem anchor; speech text is
        // rendered in the mono family. Both families are bundled with the
        // binary (see the module doc), so gpui-component's installed-font
        // probe keeps them as-is: they are explicit choices, not platform
        // defaults.
        theme.font_family = FONT_FAMILY_SANS.into();
        theme.font_size = px(font::MD);
        theme.mono_font_family = FONT_FAMILY_MONO.into();
        theme.mono_font_size = px(font::SM);

        // Radii — `--rounded-md` (12px) has no Theme field and stays a
        // component-level constant in `crate::tokens::radius`.
        theme.radius = px(radius::SM);
        theme.radius_lg = px(radius::LG);
    });
}
