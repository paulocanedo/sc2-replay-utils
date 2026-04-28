// Localization for both unit/structure names (from SC2 MPQ keys) and UI
// strings (menus, buttons, labels, tooltips, toasts).
//
// Two flat `key=value` tables per language are embedded at compile time:
//
//   data/locale/<lang>/units.txt  — MPQ key → unit/structure/research name
//   data/locale/<lang>/ui.txt     — dotted UI key → display text
//
// Lines starting with `#` and blank lines are ignored. UI values may
// contain `{name}` placeholders, substituted at runtime by `tf()`.
//
// Fallback chain for both tables: current language → English → raw key.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

// ── Embedded locale files ───────────────────────────────────────────
const EN_UNITS_RAW: &str = include_str!("../../data/locale/en/units.txt");
const PT_BR_UNITS_RAW: &str = include_str!("../../data/locale/pt-BR/units.txt");
const EN_UI_RAW: &str = include_str!("../../data/locale/en/ui.txt");
const PT_BR_UI_RAW: &str = include_str!("../../data/locale/pt-BR/ui.txt");

/// Supported languages. The enum name maps to the directory under
/// `data/locale/` via `Language::dir()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    English,
    PortugueseBR,
}

impl Language {
    /// Label shown in the language combobox (stays in its native form
    /// so users can recognize their own language).
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::PortugueseBR => "Português (BR)",
        }
    }

    /// All supported languages, in display order.
    pub fn all() -> &'static [Language] {
        &[Language::English, Language::PortugueseBR]
    }
}

// ── Parsed tables (lazy, once) ──────────────────────────────────────
type Table = HashMap<&'static str, &'static str>;

fn en_units() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(EN_UNITS_RAW))
}

fn pt_br_units() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(PT_BR_UNITS_RAW))
}

fn en_ui() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(EN_UI_RAW))
}

fn pt_br_ui() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(PT_BR_UI_RAW))
}

fn units_for(lang: Language) -> &'static Table {
    match lang {
        Language::English => en_units(),
        Language::PortugueseBR => pt_br_units(),
    }
}

fn ui_for(lang: Language) -> &'static Table {
    match lang {
        Language::English => en_ui(),
        Language::PortugueseBR => pt_br_ui(),
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Localized unit/structure/research name for a raw SC2 MPQ key.
///
/// Fallback chain: `lang` → English → raw key.
pub fn localize<'a>(key: &'a str, lang: Language) -> &'a str {
    if let Some(v) = units_for(lang).get(key) {
        return v;
    }
    if lang != Language::English {
        if let Some(v) = en_units().get(key) {
            return v;
        }
    }
    key
}

/// Localized UI string for a dotted key (e.g. `menu.file.open`).
///
/// Fallback chain: `lang` → English → raw key. For keys containing
/// `{name}` placeholders, use [`tf`] instead.
pub fn t(key: &str, lang: Language) -> &'static str {
    if let Some(v) = ui_for(lang).get(key) {
        return v;
    }
    if lang != Language::English {
        if let Some(v) = en_ui().get(key) {
            return v;
        }
    }
    // Last resort: leak the key through as a static — the caller may
    // have typoed. We still need a &'static str, and unknown keys are
    // rare, so we allocate and leak (string table is tiny).
    Box::leak(key.to_string().into_boxed_str())
}

/// Localized UI string with `{name}` placeholder substitution.
///
/// Example: `tf("toast.save_error", lang, &[("err", &msg)])`.
///
/// Also translates the literal two-character sequence `\n` into a real
/// newline so multi-line tooltip strings can live on a single line of
/// the `.txt` file.
pub fn tf(key: &str, lang: Language, args: &[(&str, &str)]) -> String {
    let mut s = t(key, lang).to_string();
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    if s.contains("\\n") {
        s = s.replace("\\n", "\n");
    }
    s
}

// ── Parser ──────────────────────────────────────────────────────────
fn parse(raw: &'static str) -> Table {
    let mut map = Table::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Use the original (non-stripped) line so trailing whitespace
        // inside values (e.g. " YOU ") is preserved; only trim the key.
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            if key.is_empty() {
                continue;
            }
            // Trim only the leading space after `=` (one space), and
            // the trailing newline — but preserve intentional padding.
            let value = v.strip_prefix(' ').unwrap_or(v);
            let value = value.trim_end_matches(['\r', '\n']);
            map.insert(key, value);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_en_basics() {
        assert_eq!(localize("Marine", Language::English), "Marine");
        assert_eq!(localize("WidowMine", Language::English), "Widow Mine");
        assert_eq!(localize("GhostAlternate", Language::English), "Ghost");
    }

    #[test]
    fn units_pt_br_basics() {
        assert_eq!(localize("Marine", Language::PortugueseBR), "Soldado");
        assert_eq!(localize("WidowMine", Language::PortugueseBR), "Mina Viúva");
        assert_eq!(localize("Probe", Language::PortugueseBR), "Sonda");
    }

    #[test]
    fn units_fallback_to_key() {
        assert_eq!(localize("UnknownUnit123", Language::English), "UnknownUnit123");
        assert_eq!(localize("UnknownUnit123", Language::PortugueseBR), "UnknownUnit123");
    }

    #[test]
    fn ui_en_basics() {
        assert_eq!(t("menu.tooltip", Language::English), "Menu");
        assert_eq!(t("menu.file.open", Language::English), "Open replay…");
        assert_eq!(t("tab.timeline", Language::English), "Timeline");
    }

    #[test]
    fn ui_pt_br_basics() {
        assert_eq!(t("menu.tooltip", Language::PortugueseBR), "Menu");
        assert_eq!(t("menu.file.open", Language::PortugueseBR), "Abrir replay…");
        assert_eq!(t("tab.charts", Language::PortugueseBR), "Gráficos");
    }

    #[test]
    fn ui_placeholder_substitution() {
        let s = tf(
            "toast.save_error",
            Language::English,
            &[("err", "disk full")],
        );
        assert_eq!(s, "Save error: disk full");
        let s = tf(
            "toast.new_replay_loaded",
            Language::PortugueseBR,
            &[("file", "foo.SC2Replay")],
        );
        assert_eq!(s, "Novo replay carregado: foo.SC2Replay");
    }

    #[test]
    fn ui_newline_escape_decoded() {
        let s = tf(
            "charts.tooltip.army_anon",
            Language::English,
            &[("mm", "5"), ("ss", "30"), ("value", "1.234")],
        );
        assert!(s.contains('\n'), "expected real newline, got: {s:?}");
        assert!(s.starts_with("Time: 5:30"));
    }
}
