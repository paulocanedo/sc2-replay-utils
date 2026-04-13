// Tipos de saída do parser single-pass.
//
// O vocabulário aqui é o que o resto do app vê — os eventos crus do
// replay (UnitInit/Born/Done/Died/TypeChange) são traduzidos para
// `EntityEvent { kind: ProductionStarted | ProductionFinished |
// ProductionCancelled | Died, … }` no `tracker` antes de chegarem
// nestas estruturas.

use std::collections::HashMap;

/// Sentinel usado em `EntityEvent.creator_ability` para marcar
/// `ProductionStarted` derivados de `UnitInit` (warp-ins / construções
/// iniciadas). O build_order usa esse marcador para distinguir
/// eventos de início de construção dos spawns "instantâneos" via
/// `UnitBorn`.
pub const UNIT_INIT_MARKER: &str = "__UnitInit__";

#[derive(Clone)]
pub struct StatsSnapshot {
    pub game_loop: u32,
    pub minerals: i32,
    pub vespene: i32,
    pub minerals_rate: i32,
    pub vespene_rate: i32,
    pub workers: i32,
    pub supply_used: i32,
    pub supply_made: i32,
    pub army_value_minerals: i32,
    pub army_value_vespene: i32,
    pub minerals_lost_army: i32,
    pub vespene_lost_army: i32,
    pub minerals_killed_army: i32,
    pub vespene_killed_army: i32,
}

#[derive(Clone)]
pub struct UpgradeEntry {
    pub game_loop: u32,
    /// Sequência global do evento na stream do tracker — ver `EntityEvent::seq`.
    pub seq: u32,
    pub name: String,
}

/// Comando de produção emitido pelo jogador (treinar unidade,
/// pesquisar upgrade, morphar prédio). Capturado de
/// `replay.game.events` (Cmd events). O `build_order` usa estes
/// registros como o instante de início real do produtor — o
/// `finish_loop` observado já reflete qualquer aceleração de Chrono
/// Boost, então não precisamos detectar janelas de chrono.
#[derive(Clone)]
pub struct ProductionCmd {
    pub game_loop: u32,
    /// Nome bruto da ability (ex. "TrainZealot",
    /// "ResearchProtossGroundWeaponsLevel1", "MorphToWarpGate").
    pub ability: String,
    /// Tags candidatos a produtor — snapshot da seleção ativa do
    /// jogador no instante do cmd. Cada elemento é o `unit_tag_index`
    /// (parte alta de um tag completo, comum entre eventos de game e
    /// tracker). Vazio quando a seleção não pôde ser resolvida (cmd
    /// órfão). O build_order escolhe o primeiro disponível pra
    /// associar via FIFO.
    pub producer_tag_indexes: Vec<u32>,
    /// `true` quando este cmd já foi consumido pelo build_order.
    /// Mantido pra permitir varreduras múltiplas (unidades agrupadas
    /// por produtor + upgrades) sem reprocessar.
    pub consumed: bool,
}

/// Tipo semântico de evento sobre unidades e estruturas.
///
/// O parser traduz os eventos crus do replay para uma destas variantes
/// — o resto do app só lida com este vocabulário, não com Born/Init/
/// Done/Died direto do MPQ.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntityEventKind {
    /// Build/train/warp/morph foi iniciado.
    ProductionStarted,
    /// Build/train/warp/morph ficou pronto.
    ProductionFinished,
    /// Build iniciado mas nunca terminou (entidade morreu antes da
    /// conclusão). Não conta como "morte" para o contador de unidades
    /// vivas.
    ProductionCancelled,
    /// Entidade pronta foi destruída ou se transformou em outro tipo
    /// (morphs emitem `Died` para o tipo antigo).
    Died,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntityCategory {
    Worker,
    Unit,
    Structure,
}

#[derive(Clone)]
pub struct EntityEvent {
    pub game_loop: u32,
    /// Sequência global do evento na stream do tracker, atribuída pelo
    /// parser. Permite reconstituir a ordem original entre `entity_events`
    /// e `upgrades` quando os dois ocorrem no mesmo `game_loop` (o
    /// build_order depende dessa interleavação).
    pub seq: u32,
    pub kind: EntityEventKind,
    pub entity_type: String,
    pub category: EntityCategory,
    /// Tag interno do replay — para correlação entre eventos do mesmo
    /// objeto.
    pub tag: i64,
    pub pos_x: u8,
    pub pos_y: u8,
    /// Habilidade que iniciou a produção. Só populado em
    /// `ProductionStarted`. Usado pelo build_order para distinguir
    /// trains/morphs de spawns iniciais.
    pub creator_ability: Option<String>,
    /// Tag completo do produtor (Gateway/Robo/Stargate/Nexus/CC/etc.)
    /// vindo de `UnitBornEvent.creator_unit_tag_*`. Só populado em
    /// `ProductionStarted` originado de Train/Morph (e em morphs
    /// in-place o produtor é o próprio tag). None para `UnitInit`
    /// (warp-in / construção via worker) e spawns iniciais — o
    /// build_order usa esse None pra cair no fallback antigo.
    pub creator_tag: Option<i64>,
    /// Quem matou a entidade. None quando o evento é uma transformação
    /// (morph) ou quando o killer é desconhecido.
    pub killer_player_id: Option<u8>,
}

/// Amostra de posição de uma unidade num instante específico, vinda
/// dos `UnitPositionsEvent` que o tracker emite periodicamente. As
/// coordenadas estão na mesma escala (células de tile) que
/// `EntityEvent.pos_x/pos_y`, então um consumer pode trocar uma pela
/// outra sem conversão.
#[derive(Clone, Copy)]
pub struct UnitPositionSample {
    pub game_loop: u32,
    /// Tag completo (`unit_tag(index, recycle)`) — casa com o `tag`
    /// dos `EntityEvent`s do mesmo objeto.
    pub tag: i64,
    pub x: u8,
    pub y: u8,
}

/// Posição da câmera de um jogador num instante específico, extraída
/// dos `CameraUpdateEvent` dos game events. Coordenadas em células de
/// tile (mesma escala que `EntityEvent.pos_x/pos_y`). Deduplicado no
/// parser: entradas consecutivas com mesma posição são descartadas.
#[derive(Clone, Copy)]
pub struct CameraPosition {
    pub game_loop: u32,
    pub x: u8,
    pub y: u8,
}

#[derive(Clone)]
pub struct ChatEntry {
    pub game_loop: u32,
    pub player_name: String,
    pub recipient: String,
    pub message: String,
}

/// Comando de Inject Larva (SpawnLarva) capturado dos game events.
/// Cada instância representa uma Queen clicando inject numa Hatchery/
/// Lair/Hive específica.
#[derive(Clone)]
pub struct InjectCmd {
    pub game_loop: u32,
    /// `unit_tag_index` da Hatchery/Lair/Hive alvo.
    pub target_tag_index: u32,
    /// Tipo resolvido do alvo: "Hatchery", "Lair" ou "Hive".
    pub target_type: String,
    /// Posição do alvo em coordenadas de tile (mesma escala que
    /// `EntityEvent.pos_x/pos_y`).
    pub target_x: u8,
    pub target_y: u8,
}

pub struct PlayerTimeline {
    pub name: String,
    pub clan: String,
    pub race: String,
    pub mmr: Option<i32>,
    /// `player_id` 1-baseado do replay (mesmo usado em
    /// `killer_player_id` dos `EntityEvent`). Precisamos dele para
    /// distinguir "cancelei meu próprio prédio" (killer == self) de
    /// "inimigo derrubou meu prédio em construção" (killer != self)
    /// — os dois casos chegam ao parser como o mesmo `UnitDied` e só
    /// o campo killer diferencia.
    pub player_id: u8,
    pub result: Option<String>,

    pub stats: Vec<StatsSnapshot>,
    pub upgrades: Vec<UpgradeEntry>,
    pub entity_events: Vec<EntityEvent>,

    /// Comandos de produção do jogador (Train/Research/Morph),
    /// ordenados por `game_loop`. Vêm de `replay.game.events` e são
    /// consumidos pelo `build_order` pra computar o instante de início
    /// real (cobrindo Chrono Boost, supply block e idle gaps sem
    /// estimativa). Vazio em fast-path / quando game events não foram
    /// processados.
    pub production_cmds: Vec<ProductionCmd>,

    /// Comandos de Inject Larva (SpawnLarva) emitidos pelo jogador,
    /// ordenados por `game_loop`. Cada entrada indica qual Hatchery/
    /// Lair/Hive foi alvo da injeção. Usado pelo `build_order` para
    /// gerar entradas de inject na tabela.
    pub inject_cmds: Vec<InjectCmd>,

    /// Amostras periódicas de posição de unidades vivas, vindas dos
    /// `UnitPositionsEvent` do tracker. Ordenado por `game_loop`. Só
    /// inclui unidades cujo `unit_tag_index` foi atribuído a este
    /// jogador via `UnitInit`/`UnitBorn`. Estruturas raramente
    /// aparecem aqui — o SC2 só amostra unidades móveis/visíveis.
    pub unit_positions: Vec<UnitPositionSample>,

    /// Posições da câmera do jogador, extraídas dos `CameraUpdateEvent`
    /// dos game events. Ordenado por `game_loop`, deduplicado por
    /// posição (entradas consecutivas com mesmo (x,y) são descartadas
    /// no parser). Usado pela aba Timeline para desenhar o viewport
    /// da câmera no minimapa.
    pub camera_positions: Vec<CameraPosition>,

    /// Diff cumulativo de "entidades vivas" por tipo. Para cada
    /// `entity_type`, um vetor ordenado de
    /// `(game_loop, alive_count_apos_o_evento)`. Construído no
    /// pós-processamento a partir de `entity_events`.
    pub alive_count: HashMap<String, Vec<(u32, i32)>>,

    /// Capacidade de produção de workers (CC/Orbital/PF/Nexus). Cada
    /// par é `(game_loop, delta)`, ordenado.
    pub worker_capacity: Vec<(u32, i32)>,

    /// game_loops em que workers (SCV/Probe) nasceram, ordenado.
    /// Usado por `production_gap` para detectar slots ociosos.
    pub worker_births: Vec<u32>,

    /// `(game_loop, attack_level_apos, armor_level_apos)` cumulativo
    /// para queries de scrubbing.
    pub upgrade_cumulative: Vec<(u32, u8, u8)>,
}

pub struct ReplayTimeline {
    pub file: String,
    pub map: String,
    pub datetime: String,
    pub game_loops: u32,
    pub duration_seconds: u32,
    pub loops_per_second: f64,
    /// `m_base_build` do header do replay — versão do protocolo que
    /// gerou o arquivo. Usado pelo `balance_data` para selecionar a
    /// tabela de tempos correspondente ao patch do replay.
    pub base_build: u32,
    /// Limite de coleta de eventos em segundos. 0 indica sem limite.
    pub max_time_seconds: u32,
    pub players: Vec<PlayerTimeline>,
    pub chat: Vec<ChatEntry>,
    /// `m_cacheHandles` do replay — cada string é um handle de 80 chars
    /// hex (4 bytes ext + 4 hex delimiter + 4 hex region + 64 hex hash).
    /// O primeiro com extensão `s2ma` aponta para o arquivo do mapa no
    /// Battle.net Cache; usado por `map_image::load_for_replay` para
    /// resolver mapas de ladder sem scan de diretório.
    pub cache_handles: Vec<String>,
    /// Dimensões do mapa em **células de tile**, vindas de
    /// `init_data.sync_lobby_state.game_description.map_size_x/y`. As
    /// posições nos tracker events (`UnitBornEvent.x/.y`, etc.) são
    /// `u8` no mesmo sistema de coordenadas, então é por isso que a
    /// renderização do mini-mapa precisa dividir por estes valores
    /// (e não por 255). Zero indica que o `init_data` faltou.
    pub map_size_x: u8,
    pub map_size_y: u8,
}
