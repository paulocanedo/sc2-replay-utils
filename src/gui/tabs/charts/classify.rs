// Classificação de tipos de unidade, nomes canônicos e paleta de cores.

use egui::Color32;

use crate::replay::{is_structure_name, is_worker_name};

/// Custo em minerals de um worker (SCV / Probe / Drone).
pub(super) const WORKER_MINERAL_COST: i32 = 50;

/// Passo da grade de amostragem do plot, em segundos. Suaviza a
/// visualização quando há muitos eventos em sucessão rápida.
pub(super) const SAMPLE_STEP_SECS: u32 = 5;

// ── Classificação de tipos ──────────────────────────────────────────

/// Categoria de um `entity_type` para fins do plot. Não depende do
/// parser — usa as mesmas heurísticas de nome que `replay::classify`
/// expõe, mais um check extra para tumors (que o parser trata como
/// estrutura, mas `is_structure_name` não reconhece pelo nome).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeKind {
    Worker,
    Army,
    /// Estrutura ou tumor — excluído do plot.
    Skip,
}

pub(super) fn type_kind(name: &str) -> TypeKind {
    if is_worker_name(name) {
        TypeKind::Worker
    } else if is_structure_name(name) || name.starts_with("CreepTumor") {
        TypeKind::Skip
    } else {
        TypeKind::Army
    }
}

/// Nome canônico de uma unidade quando ela tem forma alternativa
/// (siege mode, burrow, transformação Hellion↔Hellbat, etc.). No
/// `alive_count` essas formas aparecem como chaves separadas — por
/// exemplo um Observer que entra em Surveillance Mode some como
/// `Observer` e ressurge como `ObserverSiegeMode`. Para o gráfico
/// "por tipo", tratamos os dois como a mesma unidade.
///
/// Retorna o próprio nome quando não há alias conhecido. A canonical
/// é deliberadamente o nome **ativo** (Observer, não
/// ObserverSiegeMode) pra bater com o `units.txt` do locale.
pub(super) fn canonical_unit_name(name: &str) -> &str {
    match name {
        "ObserverSiegeMode" => "Observer",
        "OverseerSiegeMode" => "Overseer",
        "SiegeTankSieged" => "SiegeTank",
        "VikingAssault" => "VikingFighter",
        "WidowMineBurrowed" => "WidowMine",
        "Hellbat" => "Hellion",
        "WarpPrismPhasing" => "WarpPrism",
        "RoachBurrowed" => "Roach",
        "ZerglingBurrowed" => "Zergling",
        "BanelingBurrowed" => "Baneling",
        "InfestorBurrowed" => "Infestor",
        "LurkerMPBurrowed" => "LurkerMP",
        "SwarmHostBurrowedMP" => "SwarmHostMP",
        "QueenBurrowed" => "Queen",
        "UltraliskBurrowed" => "Ultralisk",
        "HydraliskBurrowed" => "Hydralisk",
        "DroneBurrowed" => "Drone",
        other => other,
    }
}

// ── Queries sobre `alive_count` ─────────────────────────────────────

/// Contagem de entidades vivas de `entity_type` no instante
/// `game_loop`, via binary search em `series` (ordenado por loop).
pub(super) fn alive_at(series: &[(u32, i32)], game_loop: u32) -> i32 {
    let i = series.partition_point(|(l, _)| *l <= game_loop);
    if i == 0 { 0 } else { series[i - 1].1.max(0) }
}

/// Valor em minerals+gas do `ArmySnapshot` mais recente em ou antes de
/// `game_loop`. Usa busca linear porque `snapshots` é curto e a função
/// é chamada uma vez por ponto amostrado — binary search seria overkill.
pub(super) fn snapshot_at<'a>(
    snaps: &'a [crate::army_value::ArmySnapshot],
    game_loop: u32,
) -> Option<&'a crate::army_value::ArmySnapshot> {
    let i = snaps.partition_point(|s| s.game_loop <= game_loop);
    if i == 0 { None } else { snaps.get(i - 1) }
}

/// Paleta de cores estável por nome de tipo. Hash simples (FNV-1a) sobre
/// o nome bruto para escolher um índice na paleta — mesmo tipo sempre
/// recebe a mesma cor, independente da ordem no HashMap ou do idioma.
pub(super) fn type_palette_color(name: &str) -> Color32 {
    // Paleta com cores vibrantes e diferenciáveis em fundo escuro.
    // Evita vermelho/azul puros (reservados para P1/P2 no modo agregado).
    const PALETTE: &[Color32] = &[
        Color32::from_rgb(0xFF, 0xB3, 0x00), // âmbar
        Color32::from_rgb(0x00, 0xC8, 0x96), // turquesa
        Color32::from_rgb(0xC4, 0x7B, 0xFF), // lilás
        Color32::from_rgb(0xFF, 0x8E, 0x8E), // coral
        Color32::from_rgb(0x66, 0xD9, 0xEF), // ciano claro
        Color32::from_rgb(0xA6, 0xE2, 0x2E), // lima
        Color32::from_rgb(0xFF, 0x6B, 0xB5), // rosa quente
        Color32::from_rgb(0xFD, 0x97, 0x1F), // laranja
        Color32::from_rgb(0x81, 0xB0, 0xFF), // azul claro
        Color32::from_rgb(0xE8, 0xE0, 0x6F), // amarelo pálido
        Color32::from_rgb(0xB2, 0xD0, 0x7F), // verde oliva
        Color32::from_rgb(0xD7, 0x99, 0x70), // bronze
    ];
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    PALETTE[(h as usize) % PALETTE.len()]
}
