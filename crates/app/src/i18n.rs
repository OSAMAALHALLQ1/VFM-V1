//! Tiny i18n layer. All user-facing text comes from `design/strings.json`.
//! Arabic enables full RTL mirroring; timecodes stay LTR by design.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    Ar,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ar => "ar",
        }
    }

    pub fn rtl(self) -> bool {
        self == Self::Ar
    }

    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("ar") {
            Self::Ar
        } else {
            Self::En
        }
    }
}

#[derive(Debug, Deserialize)]
struct File {
    en: HashMap<String, String>,
    ar: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    strings: HashMap<String, String>,
    locale: Locale,
}

impl Catalog {
    pub fn load(locale: Locale) -> Self {
        let file: File = serde_json::from_str(include_str!("../../../design/strings.json"))
            .expect("design/strings.json must be valid");
        let strings = match locale {
            Locale::En => file.en,
            Locale::Ar => file.ar,
        };
        Self { strings, locale }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn get(&self, key: &str) -> &str {
        self.strings
            .get(key)
            .unwrap_or_else(|| panic!("missing i18n key: {key}"))
    }
}

/// Applies the catalog to the generated Slint `Strings` global.
pub fn apply(global: &crate::Strings<'_>, catalog: &Catalog) {
    global.set_gallery_title(catalog.get("gallery-title").into());
    global.set_theme_label(catalog.get("theme-label").into());
    global.set_language_label(catalog.get("language-label").into());
    global.set_accent_label(catalog.get("accent-label").into());
    global.set_dark(catalog.get("dark").into());
    global.set_light(catalog.get("light").into());
    global.set_arabic(catalog.get("arabic").into());
    global.set_english(catalog.get("english").into());
    global.set_section_buttons(catalog.get("section-buttons").into());
    global.set_section_sliders(catalog.get("section-sliders").into());
    global.set_section_inputs(catalog.get("section-inputs").into());
    global.set_section_collections(catalog.get("section-collections").into());
    global.set_section_overlays(catalog.get("section-overlays").into());
    global.set_section_icons(catalog.get("section-icons").into());
    global.set_primary(catalog.get("primary").into());
    global.set_secondary(catalog.get("secondary").into());
    global.set_disabled(catalog.get("disabled").into());
    global.set_toggle(catalog.get("toggle").into());
    global.set_volume(catalog.get("volume").into());
    global.set_seek(catalog.get("seek").into());
    global.set_search(catalog.get("search").into());
    global.set_tabs(catalog.get("tabs").into());
    global.set_row_title(catalog.get("row-title").into());
    global.set_row_subtitle(catalog.get("row-subtitle").into());
    global.set_card_title(catalog.get("card-title").into());
    global.set_card_caption(catalog.get("card-caption").into());
    global.set_context_menu(catalog.get("context-menu").into());
    global.set_dialog(catalog.get("dialog").into());
    global.set_toast(catalog.get("toast").into());
    global.set_menu_play(catalog.get("menu-play").into());
    global.set_menu_queue(catalog.get("menu-queue").into());
    global.set_menu_hide(catalog.get("menu-hide").into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_locales_cover_the_same_keys() {
        let en = Catalog::load(Locale::En);
        let ar = Catalog::load(Locale::Ar);
        assert_eq!(en.strings.len(), ar.strings.len());
        for key in en.strings.keys() {
            assert!(ar.strings.contains_key(key), "Arabic missing {key}");
        }
    }

    #[test]
    fn locale_drives_rtl() {
        assert!(!Locale::En.rtl());
        assert!(Locale::Ar.rtl());
        assert_eq!(Locale::parse("ar"), Locale::Ar);
        assert_eq!(Locale::parse("en"), Locale::En);
    }
}
