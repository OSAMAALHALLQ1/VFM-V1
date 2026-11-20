//! Hidden component gallery for reviewing the design system against Figma.

slint::include_modules!();

#[path = "../i18n.rs"]
mod i18n;

use std::env;

use i18n::{Catalog, Locale};
use slint::{Color, SharedString};

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut locale = Locale::En;
    let mut light = false;
    let mut accent: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lang" | "--locale" => {
                if let Some(v) = args.get(i + 1) {
                    locale = Locale::parse(v);
                }
                i += 1;
            }
            "--rtl" => locale = Locale::Ar,
            "--light" => light = true,
            "--dark" => light = false,
            "--accent" => {
                if let Some(v) = args.get(i + 1) {
                    accent = Some(v);
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let ui = GalleryWindow::new()?;
    apply_locale(&ui, locale);
    if light {
        ui.global::<AppTheme>().set_mode("light".into());
    }
    if let Some(hex) = accent.and_then(parse_hex) {
        ui.global::<AppTheme>().set_accent(hex);
    }

    let ui_weak = ui.as_weak();
    ui.on_set_locale(move |code: SharedString| {
        if let Some(ui) = ui_weak.upgrade() {
            apply_locale(&ui, Locale::parse(&code));
        }
    });
    ui.run()
}

fn apply_locale(ui: &GalleryWindow, locale: Locale) {
    let catalog = Catalog::load(locale);
    i18n::apply(&ui.global::<Strings>(), &catalog);
    let theme = ui.global::<AppTheme>();
    theme.set_locale(catalog.locale().code().into());
    theme.set_rtl(catalog.locale().rtl());
    theme.set_font_family(
        if catalog.locale().rtl() {
            "Noto Sans Arabic"
        } else {
            "Inter"
        }
        .into(),
    );
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_argb_u8(255, r, g, b))
}
