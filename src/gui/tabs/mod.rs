// Macro-abas de conteúdo do app.

pub mod build_order;
pub mod chat;
pub mod charts;
pub mod timeline;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Tab {
    Timeline,
    BuildOrder,
    Charts,
    Chat,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Timeline => "Timeline",
            Tab::BuildOrder => "Build Order",
            Tab::Charts => "Gráficos",
            Tab::Chat => "Chat",
        }
    }

    pub const ALL: [Tab; 4] = [Tab::Timeline, Tab::BuildOrder, Tab::Charts, Tab::Chat];
}
