// Localização de nomes de unidades, estruturas e pesquisas do SC2.
//
// Carrega arquivos de texto puro `data/locale/<lang>.txt` embutidos em
// compile-time via `include_str!`. Cada arquivo contém linhas no formato:
//
//   ChaveMpq=Nome Exibido
//
// Linhas vazias e linhas começando com `#` são ignoradas.
//
// Fallback: idioma atual → inglês → chave MPQ crua (passthrough).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

// ── Arquivos de locale embutidos ────────────────────────────────────
const EN_RAW: &str = include_str!("../../data/locale/en.txt");
const PT_BR_RAW: &str = include_str!("../../data/locale/pt-BR.txt");

/// Idiomas suportados. O valor de cada variante deve bater com o nome
/// do arquivo (sem extensão) em `data/locale/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    English,
    PortugueseBR,
}

impl Language {
    /// Label exibido no combo-box de seleção de idioma.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::PortugueseBR => "Português (BR)",
        }
    }

    /// Todas as variantes, na ordem do combo-box.
    pub fn all() -> &'static [Language] {
        &[Language::English, Language::PortugueseBR]
    }
}

// ── Tabelas parseadas (lazy, uma única vez) ─────────────────────────
type Table = HashMap<&'static str, &'static str>;

fn en_table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(EN_RAW))
}

fn pt_br_table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(PT_BR_RAW))
}

fn table_for(lang: Language) -> &'static Table {
    match lang {
        Language::English => en_table(),
        Language::PortugueseBR => pt_br_table(),
    }
}

// ── API pública ─────────────────────────────────────────────────────

/// Retorna o nome localizado para a chave MPQ fornecida.
///
/// Cadeia de fallback: `lang` → English → chave crua.
pub fn localize<'a>(key: &'a str, lang: Language) -> &'a str {
    if let Some(v) = table_for(lang).get(key) {
        // SAFETY: &'static str pode ser retornado como &'a str
        return v;
    }
    // Fallback para inglês
    if lang != Language::English {
        if let Some(v) = en_table().get(key) {
            return v;
        }
    }
    // Último recurso: chave MPQ crua
    key
}

// ── Parser ──────────────────────────────────────────────────────────
fn parse(raw: &'static str) -> Table {
    let mut map = Table::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            map.insert(k.trim(), v.trim());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_basics() {
        assert_eq!(localize("Marine", Language::English), "Marine");
        assert_eq!(localize("WidowMine", Language::English), "Widow Mine");
        assert_eq!(localize("GhostAlternate", Language::English), "Ghost");
    }

    #[test]
    fn pt_br_basics() {
        assert_eq!(localize("Marine", Language::PortugueseBR), "Soldado");
        assert_eq!(localize("WidowMine", Language::PortugueseBR), "Mina Viúva");
        assert_eq!(localize("Probe", Language::PortugueseBR), "Sonda");
    }

    #[test]
    fn fallback_to_key() {
        // Chave inexistente → retorna a própria chave
        assert_eq!(localize("UnknownUnit123", Language::English), "UnknownUnit123");
        assert_eq!(localize("UnknownUnit123", Language::PortugueseBR), "UnknownUnit123");
    }
}
