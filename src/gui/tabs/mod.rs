// Macro-abas de conteúdo do app.

pub mod build_order;
pub mod chat;
pub mod charts;
pub mod insights;
pub mod timeline;

use crate::locale::{t, Language};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Tab {
    Timeline,
    BuildOrder,
    Charts,
    Chat,
    Insights,
}

impl Tab {
    /// Localized label for the tab header.
    pub fn label(self, lang: Language) -> &'static str {
        let key = match self {
            Tab::Timeline => "tab.timeline",
            Tab::BuildOrder => "tab.build_order",
            Tab::Charts => "tab.charts",
            Tab::Chat => "tab.chat",
            Tab::Insights => "tab.insights",
        };
        t(key, lang)
    }

    pub const ALL: [Tab; 5] = [
        Tab::Timeline,
        Tab::BuildOrder,
        Tab::Charts,
        Tab::Chat,
        Tab::Insights,
    ];
}
