// Extrator de build order — agora é uma camada pura sobre `ReplayTimeline`.
//
// Não abre o MPQ nem decodifica eventos: consome `entity_events` e
// `upgrades` que o parser single-pass já produziu, mapeando cada um
// para `BuildOrderEntry` na semântica esperada pelos consumers
// (CSV, GUI, image renderer).

use crate::replay::{
    EntityCategory, EntityEventKind, PlayerTimeline, ReplayTimeline, UNIT_INIT_MARKER,
};

// ── Structs de saída ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BuildOrderEntry {
    pub supply: u8,
    pub game_loop: u32,
    /// Sequência global vinda do parser, usada como tiebreaker entre
    /// `entity_events` e `upgrades` no mesmo `game_loop`. Não é
    /// exposto no CSV.
    pub seq: u32,
    pub action: String,
    pub count: u32,
    pub is_upgrade: bool,
    pub is_structure: bool,
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

// ── Extração ──────────────────────────────────────────────────────────────────

/// Constrói o `BuildOrderResult` a partir de um `ReplayTimeline` já
/// parseado. Chama O(eventos), sem I/O.
pub fn extract_build_order(timeline: &ReplayTimeline) -> Result<BuildOrderResult, String> {
    let players = timeline
        .players
        .iter()
        .map(|p| PlayerBuildOrder {
            name: p.name.clone(),
            race: p.race.clone(),
            mmr: p.mmr,
            entries: build_player_entries(p),
        })
        .collect();

    Ok(BuildOrderResult {
        players,
        datetime: timeline.datetime.clone(),
        map_name: timeline.map.clone(),
        loops_per_second: timeline.loops_per_second,
    })
}

fn build_player_entries(player: &PlayerTimeline) -> Vec<BuildOrderEntry> {
    let mut raw: Vec<BuildOrderEntry> = Vec::new();

    // Entidades — só ProductionStarted, filtrado por origem da habilidade.
    for ev in &player.entity_events {
        if ev.kind != EntityEventKind::ProductionStarted {
            continue;
        }
        if ev.game_loop == 0 {
            continue;
        }
        let Some(ability) = ev.creator_ability.as_deref() else {
            // Sem ability associada → spawn inicial / coisa fora de
            // build order (CC inicial, larvas, etc.).
            continue;
        };

        let from_unit_init = ability == UNIT_INIT_MARKER;
        let from_train = ability.contains("Train");
        let from_morph = ability.starts_with("MorphTo");
        if !from_unit_init && !from_train && !from_morph {
            continue;
        }

        let supply = player
            .stats_at(ev.game_loop)
            .map(|s| s.supply_used as u8)
            .unwrap_or(0);

        // is_structure: UnitInit sempre cria estrutura; morphs criam
        // estrutura quando o tipo destino é uma estrutura. Trains nunca
        // criam estrutura.
        let is_structure = from_unit_init
            || (from_morph && matches!(ev.category, EntityCategory::Structure));

        raw.push(BuildOrderEntry {
            supply,
            game_loop: ev.game_loop,
            seq: ev.seq,
            action: ev.entity_type.clone(),
            count: 1,
            is_upgrade: false,
            is_structure,
        });
    }

    // Upgrades — Sprays já filtrados pelo parser.
    for u in &player.upgrades {
        if u.game_loop == 0 {
            continue;
        }
        let supply = player
            .stats_at(u.game_loop)
            .map(|s| s.supply_used as u8)
            .unwrap_or(0);
        raw.push(BuildOrderEntry {
            supply,
            game_loop: u.game_loop,
            seq: u.seq,
            action: u.name.clone(),
            count: 1,
            is_upgrade: true,
            is_structure: false,
        });
    }

    // Sort por (game_loop, seq) reconstrói a interleavação original
    // do tracker entre entidades e upgrades.
    raw.sort_by_key(|e| (e.game_loop, e.seq));

    deduplicate(raw)
}

/// Funde entradas consecutivas com a mesma ação em uma única com `count` incrementado.
fn deduplicate(entries: Vec<BuildOrderEntry>) -> Vec<BuildOrderEntry> {
    let mut out: Vec<BuildOrderEntry> = Vec::new();
    for entry in entries {
        match out.last_mut() {
            Some(last) if last.action == entry.action => last.count += 1,
            _ => out.push(entry),
        }
    }
    out
}

// ── Classificação de entradas ─────────────────────────────────────────────────

/// Categoria de uma entrada do build order. `Worker` é um subtipo
/// especial de `Unit` para SCV/Probe/Drone/MULE — útil pra filtros de
/// UI que querem esconder spam de workers sem sumir com o resto das
/// unidades. `Research` vs `Upgrade` distingue pesquisas pontuais
/// (Stimpack, Blink, WarpGate…) de upgrades com níveis
/// (InfantryWeaponsLevel1/2/3, Armor…).
#[allow(dead_code)] // consumido apenas pelo binário GUI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Worker,
    Unit,
    Structure,
    Research,
    Upgrade,
}

#[allow(dead_code)] // consumido apenas pelo binário GUI
impl EntryKind {
    /// Letra curta usada em UIs compactas (coluna "tipo" na GUI).
    /// `U` colide entre Unit e Upgrade — escolhemos `U` para Unit e
    /// `P` (de u**p**grade) para o segundo, já que Unit é mais comum.
    pub fn short_letter(self) -> &'static str {
        match self {
            EntryKind::Worker => "W",
            EntryKind::Unit => "U",
            EntryKind::Structure => "S",
            EntryKind::Research => "R",
            EntryKind::Upgrade => "P",
        }
    }

    /// Nome completo em inglês — útil como tooltip.
    pub fn full_name(self) -> &'static str {
        match self {
            EntryKind::Worker => "Worker",
            EntryKind::Unit => "Unit",
            EntryKind::Structure => "Structure",
            EntryKind::Research => "Research",
            EntryKind::Upgrade => "Upgrade",
        }
    }
}

/// Classifica uma entrada do build order em uma `EntryKind`. A decisão
/// usa os flags já armazenados (`is_upgrade`/`is_structure`) e o nome
/// bruto da ação para distinguir worker/unit e research/upgrade.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn classify_entry(entry: &BuildOrderEntry) -> EntryKind {
    if entry.is_upgrade {
        if is_leveled_upgrade(&entry.action) {
            EntryKind::Upgrade
        } else {
            EntryKind::Research
        }
    } else if entry.is_structure {
        EntryKind::Structure
    } else if is_worker_name(&entry.action) {
        EntryKind::Worker
    } else {
        EntryKind::Unit
    }
}

/// Retorna `true` se o nome da unidade é um worker (coletor de
/// recursos). Inclui MULE por gerar recurso como os demais, ainda
/// que seja invocado pela Orbital Command em vez de treinado.
#[allow(dead_code)] // consumido apenas pelo binário GUI
pub fn is_worker_name(name: &str) -> bool {
    matches!(name, "SCV" | "Probe" | "Drone" | "MULE")
}

/// Heurística para separar upgrades com níveis (Weapons/Armor 1-3)
/// de pesquisas pontuais. SC2 sufixa os níveis com "Level1/2/3".
#[allow(dead_code)] // consumido apenas pelo binário GUI
fn is_leveled_upgrade(name: &str) -> bool {
    name.ends_with("Level1") || name.ends_with("Level2") || name.ends_with("Level3")
}

// ── Formatação de tempo ──────────────────────────────────────────────────────

pub fn format_time(game_loop: u32, lps: f64) -> String {
    let total_secs = (game_loop as f64 / lps).round() as u32;
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}
