//! Enums e struct de filtro/ordenação da biblioteca.

#[derive(Default, PartialEq, Clone, Copy)]
pub enum OutcomeFilter {
    #[default]
    All,
    Wins,
    Losses,
}

#[derive(Default, Debug, PartialEq, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum DateRange {
    All,
    Today,
    #[default]
    ThisWeek,
    ThisMonth,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum SortOrder {
    #[default]
    Date,
    Duration,
    Mmr,
    Map,
}

pub struct LibraryFilter {
    pub search: String,
    pub race: Option<char>,
    pub outcome: OutcomeFilter,
    pub date_range: DateRange,
    pub sort: SortOrder,
    pub sort_ascending: bool,
}

impl Default for LibraryFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            race: None,
            outcome: OutcomeFilter::All,
            date_range: DateRange::default(),
            sort: SortOrder::Date,
            sort_ascending: false,
        }
    }
}

impl LibraryFilter {
    /// Inicializa o filtro restaurando preferências salvas no config.
    pub fn from_config(config: &crate::config::AppConfig) -> Self {
        Self {
            date_range: config.library_date_range,
            ..Self::default()
        }
    }
}
