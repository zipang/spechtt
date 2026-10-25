//! Theme contract: the Rust gpui-kit theme values must equal the
//! `design-tokens.css` token values.
//!
//! Mirrors `tests/design-contract.test.ts` two-layer contract:
//! 1. CSS literals == `speechtt_ui::tokens` (hard literals below),
//! 2. `speechtt_ui::tokens` == the installed gpui-kit Theme (role asserts).
//!    Update both sides together when the design changes (`design-tokens.css`,
//!    then this file), and let the tests disagree noisily in between.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Theme;
use gpui_kit::{Hsla, Rgba, TestAppContext, px, rgb};

use speechtt_ui::tokens::{color, font, radius, space};

/// Layer 1 — the tokens module mirrors `design-tokens.css` (hard literals).
#[test]
fn token_constants_mirror_design_tokens_css() {
    assert_eq!(color::BRAND_PRIMARY, 0xea5514);
    assert_eq!(color::BRAND_ACCENT, 0xff8a3d);
    assert_eq!(color::BRAND_SECONDARY, 0x2a2a32);
    assert_eq!(color::SURFACE, 0x17171b);
    assert_eq!(color::SURFACE_ALT, 0x202027);
    assert_eq!(color::SURFACE_DARK, 0x0e0e11);
    assert_eq!(color::SURFACE_CARD, 0x1c1c22);
    assert_eq!(color::SUCCESS, 0x35c48b);
    assert_eq!(color::INFO, 0x4d9dff);
    assert_eq!(color::WARNING, 0xf0a82e);
    assert_eq!(color::DANGER, 0xe5484d);
    assert_eq!(color::TEXT, 0xf2efeb);
    assert_eq!(color::TEXT_MUTED, 0x9c9aa3);
    assert_eq!(color::ON_COLOR, 0x000000);

    assert_eq!((radius::SM, radius::MD, radius::LG), (6.0, 12.0, 20.0));
    assert_eq!(
        (
            space::XS,
            space::SM,
            space::MD,
            space::BASE,
            space::LG,
            space::XL,
            space::XXL
        ),
        (4.0, 8.0, 12.0, 16.0, 20.0, 32.0, 48.0)
    );
    assert_eq!(
        (
            font::XS,
            font::SM,
            font::MD,
            font::LG,
            font::XL,
            font::TWO_XL
        ),
        (10.24, 12.8, 16.0, 20.0, 25.0, 31.25)
    );
}

/// Layer 2 helper — Hsla has no `PartialEq`; compare as RGBA components.
/// The Rgba→Hsla→Rgba round trip drifts ~1e-7, so exact f32 equality is
/// the wrong contract; a hair of tolerance still fails loudly on real
/// mismatches (a 1/255 step is 4e-3).
fn assert_role(name: &str, actual: Hsla, expected: Rgba) {
    let actual = Rgba::from(actual);
    let close = (actual.r - expected.r).abs() < 1e-3
        && (actual.g - expected.g).abs() < 1e-3
        && (actual.b - expected.b).abs() < 1e-3
        && (actual.a - expected.a).abs() < 1e-3;
    assert!(
        close,
        "theme role `{name}` must equal its design token ({expected:?} != {actual:?})"
    );
}

/// Layer 2 — `theme::apply` installs SpeechTT tokens into the gpui-kit
/// theme; semantic roles read back exactly.
#[gpui_kit::test]
fn the_active_theme_carries_the_speechtt_token_values(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);

        let theme = cx.theme();
        let c = &theme.colors;

        // Surfaces
        assert_role("background", c.background, rgb(color::SURFACE));
        assert_role("foreground", c.foreground, rgb(color::TEXT));
        assert_role("title_bar", c.title_bar, rgb(color::SURFACE_DARK));
        assert_role(
            "title_bar_border",
            c.title_bar_border,
            rgb(color::BRAND_SECONDARY),
        );
        assert_role("popover", c.popover, rgb(color::SURFACE_CARD));
        assert_role("popover_foreground", c.popover_foreground, rgb(color::TEXT));
        assert_role("secondary", c.secondary, rgb(color::SURFACE_ALT));
        assert_role(
            "secondary_foreground",
            c.secondary_foreground,
            rgb(color::TEXT),
        );
        assert_role("input", c.input, rgb(color::SURFACE_ALT));
        assert_role(
            "muted_foreground",
            c.muted_foreground,
            rgb(color::TEXT_MUTED),
        );

        // Chrome
        assert_role("border", c.border, rgb(color::BRAND_SECONDARY));
        assert_role("ring", c.ring, rgb(color::BRAND_ACCENT));

        // Primary brand
        assert_role("primary", c.primary, rgb(color::BRAND_PRIMARY));
        assert_role(
            "primary_foreground",
            c.primary_foreground,
            rgb(color::ON_COLOR),
        );
        assert_role("primary_hover", c.primary_hover, rgb(color::BRAND_ACCENT));

        // Neutral buttons
        assert_role("button", c.button, rgb(color::SURFACE_ALT));
        assert_role("button_hover", c.button_hover, rgb(color::BRAND_SECONDARY));

        // Action fills — all carry black ink
        assert_role("success", c.success, rgb(color::SUCCESS));
        assert_role(
            "success_foreground",
            c.success_foreground,
            rgb(color::ON_COLOR),
        );
        assert_role("info", c.info, rgb(color::INFO));
        assert_role("info_foreground", c.info_foreground, rgb(color::ON_COLOR));
        assert_role("warning", c.warning, rgb(color::WARNING));
        assert_role(
            "warning_foreground",
            c.warning_foreground,
            rgb(color::ON_COLOR),
        );
        assert_role("danger", c.danger, rgb(color::DANGER));
        assert_role(
            "danger_foreground",
            c.danger_foreground,
            rgb(color::ON_COLOR),
        );

        // Typography / radii — sizes and radii are exact-value contracts.
        // Font families are bundled (theme.rs embeds Inter + JetBrains
        // Mono), so they assert as explicit exact values — the probe only
        // substitutes *platform-default* families, never these.
        assert_eq!(theme.font_size, px(font::MD));
        assert_eq!(theme.mono_font_size, px(font::SM));
        assert_eq!(theme.font_family, speechtt_ui::theme::FONT_FAMILY_SANS);
        assert_eq!(theme.mono_font_family, speechtt_ui::theme::FONT_FAMILY_MONO);
        assert_eq!(theme.font_family, "Inter");
        assert_eq!(theme.mono_font_family, "JetBrains Mono");
        assert_eq!(theme.radius, px(radius::SM));
        assert_eq!(theme.radius_lg, px(radius::LG));
    });
}

/// `theme::apply` may be repeated (user re-applies, tests re-enter) without
/// drifting: a reapplied theme still equals the contract.
#[gpui_kit::test]
fn applying_the_theme_twice_is_stable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
        speechtt_ui::theme::apply(cx);

        let theme = cx.global::<Theme>();
        assert_role(
            "background (after re-apply)",
            theme.colors.background,
            rgb(color::SURFACE),
        );
    });
}

/// `Pixels` equality sanity for the base font anchor (`--font-size-md`).
#[test]
fn base_font_is_the_rem_anchor() {
    assert!((f32::from(px(font::MD)) - 16.0).abs() < f32::EPSILON);
}
