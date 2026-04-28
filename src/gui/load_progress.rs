//! Tipos compartilhados entre a UI e a worker thread que carrega o
//! replay em background.
//!
//! Filosofia: o `Sender<LoadProgress>` vive na thread; o `Receiver` vive
//! em `AppState`. A thread emite `Stage(...)` em pontos relevantes do
//! pipeline de parse + extração e finaliza com `Done(...)` ou
//! `Failed(...)`. A UI drena o canal a cada frame em `poll_load`.
//!
//! Como o callback de progresso é repassado para `replay::parse` (módulo
//! de domínio que não depende de GUI), `LoadStage` mora aqui e é
//! re-exportado tanto para a thread quanto para o parser.

use std::sync::mpsc::Receiver;

use crate::replay_state::LoadedReplay;

/// Etapas reportadas durante o carregamento de um replay.
///
/// A ordem reflete a sequência real em que cada etapa começa — usada
/// pela status bar e pela tela de loading para mostrar o que o app
/// está fazendo no momento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadStage {
    /// `std::fs::read` do arquivo .SC2Replay.
    ReadingFile,
    /// Parse do MPQ + header + details + init_data.
    ParsingHeader,
    /// `tracker::process_tracker_events` — costuma ser a etapa mais longa.
    DecodingTracker,
    /// `game::process_game_events`.
    DecodingGame,
    /// `message::process_message_events`.
    DecodingMessages,
    /// Extratores derivados: build order, army value, chat, production
    /// gaps, supply blocks.
    ExtractingFeatures,
    /// Resolução do minimapa via Battle.net Cache (somente nativo).
    LoadingMinimap,
}

impl LoadStage {
    /// Chave de locale (`load.stage.*`) para a etapa atual. Usada pela
    /// status bar e pela tela de loading central.
    pub fn locale_key(self) -> &'static str {
        match self {
            LoadStage::ReadingFile => "load.stage.reading_file",
            LoadStage::ParsingHeader => "load.stage.parsing_header",
            LoadStage::DecodingTracker => "load.stage.decoding_tracker",
            LoadStage::DecodingGame => "load.stage.decoding_game",
            LoadStage::DecodingMessages => "load.stage.decoding_messages",
            LoadStage::ExtractingFeatures => "load.stage.extracting_features",
            LoadStage::LoadingMinimap => "load.stage.loading_minimap",
        }
    }
}

/// Mensagens enviadas da worker thread para a UI.
///
/// `Done` carrega o `LoadedReplay` em `Box` para evitar mover uma
/// struct grande pelo canal a cada send.
pub enum LoadProgress {
    Stage(LoadStage),
    Done(Box<LoadedReplay>),
    Failed(String),
}

/// Estado de uma carga em andamento. Vive em `AppState::load_in_flight`.
///
/// `generation` é incrementado a cada `load_path`; uma worker thread
/// "antiga" continua rodando até o fim mas seu `Sender` fica órfão
/// quando o `Receiver` é substituído (drop), então as mensagens são
/// silenciosamente descartadas. Não precisamos checar `generation`
/// explicitamente — o drop do canal é suficiente.
pub struct LoadHandle {
    pub generation: u64,
    pub file_name: String,
    /// Última etapa recebida via `Stage(...)`. Inicializa como
    /// `ReadingFile` e avança conforme as mensagens chegam.
    pub current_stage: LoadStage,
    pub rx: Receiver<LoadProgress>,
}
