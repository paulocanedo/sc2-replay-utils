//! Tipos de saída do extrator de build order + utilitário de
//! formatação de tempo em `mm:ss`.

/// Desfecho real de uma entrada do build order. A maioria das entradas
/// são `Completed` (produção terminou normalmente). As duas outras
/// variantes só se aplicam a estruturas cujo `UnitDied` chegou antes
/// do `UnitDone`, e são distinguidas pelo `killer_player_id` do
/// `ProductionCancelled` que o parser emite nesse caso:
///
/// - `Cancelled`: jogador clicou "cancel" no prédio em construção
///   (killer é o próprio dono ou None). SC2 reembolsa 75%.
/// - `DestroyedInProgress`: o inimigo derrubou o prédio antes de
///   completar (killer é um player diferente).
///
/// Unidades/workers/upgrades que nunca chegam a ser canceláveis ficam
/// sempre como `Completed` — pra elas o `UnitDied` posterior, se
/// existir, não afeta o build order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryOutcome {
    Completed,
    Cancelled,
    DestroyedInProgress,
}

impl EntryOutcome {
    /// Letra usada na coluna `outcome` do golden CSV. `C`/`X`/`D` são
    /// escolhidas pra serem visualmente distintas num diff.
    pub fn short_letter(self) -> &'static str {
        match self {
            EntryOutcome::Completed => "C",
            EntryOutcome::Cancelled => "X",
            EntryOutcome::DestroyedInProgress => "D",
        }
    }
}

#[derive(Clone)]
pub struct BuildOrderEntry {
    /// Supply usado no instante de início.
    pub supply: u8,
    /// Capacidade total de supply no instante de início (food_made).
    pub supply_made: u8,
    /// Instante de início da ação (start time).
    pub game_loop: u32,
    /// Instante de conclusão da ação. Significado depende do `outcome`:
    /// - `Completed`: instante projetado de conclusão (start + build_time).
    /// - `Cancelled` / `DestroyedInProgress`: instante real em que o
    ///   prédio foi cancelado/destruído durante a construção.
    pub finish_loop: u32,
    /// Sequência global vinda do parser, usada como tiebreaker entre
    /// `entity_events` e `upgrades` no mesmo `game_loop`. Não é
    /// exposto no CSV.
    pub seq: u32,
    pub action: String,
    pub count: u32,
    pub is_upgrade: bool,
    pub is_structure: bool,
    pub outcome: EntryOutcome,
    /// Número estimado de Chrono Boosts que aceleraram esta produção.
    /// 0 quando não houve chrono ou quando o start_loop veio do
    /// fallback (sem cmd matching, não dá pra saber). Calculado
    /// comparando o tempo real `finish - start` com o
    /// `build_time_loops` base da ação.
    pub chrono_boosts: u8,
}

pub struct PlayerBuildOrder {
    pub name: String,
    pub race: String,
    pub mmr: Option<i32>,
    pub entries: Vec<BuildOrderEntry>,
}

pub struct BuildOrderResult {
    pub players: Vec<PlayerBuildOrder>,
    pub datetime: String,
    pub map_name: String,
    pub loops_per_second: f64,
}

pub fn format_time(game_loop: u32, lps: f64) -> String {
    let total_secs = (game_loop as f64 / lps).round() as u32;
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}
