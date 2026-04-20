// Card de insight: eficiência de inject (só Zerg).
//
// Pra cada Hatchery/Lair/Hive (agrupado por `target_tag_index`) calcula
// quantos injects o jogador fez vs o ideal — (game_end - first_inject)
// dividido pelo ciclo de 40s. Agrega por soma absoluta entre bases
// (actual / ideal), então uma base injetada 100% e outra ignorada dão
// média ponderada pelo tempo de cada uma. Não renderiza pra outras
// raças.

use std::collections::HashMap;

use egui::{Color32, RichText, Ui};

use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::LoadedReplay;
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;

/// Ciclo de inject em segundos — tempo que a Queen leva pra recarregar
/// energia suficiente (25 energy regen rate ≈ 1.4s por unidade) e
/// tempo de duração do efeito no larva spawn.
const INJECT_CYCLE_SECS: f64 = 40.0;

/// Minuto de corte: ineficiência antes desse marco custa muito mais
/// larva (cada inject perdido = ~3 drones/lings a menos num momento
/// de macro crítica). Depois disso a economia satura e a métrica
/// perde poder diagnóstico.
const CUTOFF_MINUTES: u32 = 7;

pub fn show(ui: &mut Ui, loaded: &LoadedReplay, config: &AppConfig, player_idx: usize) {
    let lang = config.language;
    let Some(player) = loaded.timeline.players.get(player_idx) else {
        return;
    };
    if !is_zerg_race(&player.race) {
        return;
    }

    let lps = loaded.timeline.loops_per_second.max(0.0001);
    let cycle_loops = INJECT_CYCLE_SECS * lps;
    let cutoff_loop = ((CUTOFF_MINUTES as f64) * 60.0 * lps).round() as u32;
    let window_end = loaded.timeline.game_loops.min(cutoff_loop);

    // Agrupa injects por base (target_tag_index), já filtrando os que
    // ocorreram após o cutoff.
    let mut per_base: HashMap<u32, Vec<u32>> = HashMap::new();
    for cmd in &player.inject_cmds {
        if cmd.game_loop > window_end {
            continue;
        }
        per_base.entry(cmd.target_tag_index).or_default().push(cmd.game_loop);
    }

    // Pra cada base: actual = injects feitos até o cutoff; ideal =
    // janela / 40s, onde a janela vai do primeiro inject até o cutoff
    // (ou fim do jogo se antes). Usar o primeiro inject como início
    // (em vez do nascimento da Hatch) evita penalizar tempo
    // "pré-queen", que é inevitável.
    let mut total_actual: u32 = 0;
    let mut total_ideal: u32 = 0;
    let bases = per_base.len() as u32;
    for loops in per_base.values() {
        let actual = loops.len() as u32;
        let first = *loops.iter().min().unwrap_or(&0);
        if window_end > first && cycle_loops > 0.0 {
            let window = (window_end - first) as f64;
            let ideal = (window / cycle_loops).floor() as u32;
            total_ideal += ideal.max(1);
        }
        total_actual += actual;
    }

    let title = tf(
        "insight.inject_efficiency.title",
        lang,
        &[("minutes", &CUTOFF_MINUTES.to_string())],
    );
    let help_text = tf(
        "insight.inject_efficiency.help",
        lang,
        &[("minutes", &CUTOFF_MINUTES.to_string())],
    );

    insight_card(ui, config, "inject_efficiency", &title, &help_text, |ui| {
        render_body(ui, config, total_actual, total_ideal, bases);
    });
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    actual: u32,
    ideal: u32,
    bases: u32,
) {
    let lang = config.language;
    let size = size_subtitle(config);

    if actual == 0 {
        ui.label(
            RichText::new(t("insight.inject_efficiency.none", lang)).italics(),
        );
        ui.add_space(SPACE_M);
        return;
    }

    // Cap em 100% — jogadores podem fazer mais injects que o ideal se
    // spawnaram bases e injetaram imediatamente, mas a métrica como
    // "uptime" estoura nesses casos.
    let efficiency = if ideal == 0 {
        100.0
    } else {
        (actual as f64 / ideal as f64 * 100.0).min(100.0)
    };
    let color = severity_color(efficiency);

    ui.vertical(|ui| {
        ui.label(
            RichText::new(t("insight.inject_efficiency.efficiency", lang)).size(size * 0.85),
        );
        ui.label(
            RichText::new(format!("{:.1}%", efficiency))
                .size(size * 1.4)
                .strong()
                .color(color),
        );
    });

    ui.add_space(SPACE_S);

    ui.label(
        RichText::new(tf(
            "insight.inject_efficiency.summary_line",
            lang,
            &[
                ("actual", &actual.to_string()),
                ("bases", &bases.to_string()),
                ("ideal", &ideal.to_string()),
            ],
        ))
        .italics(),
    );

    ui.add_space(SPACE_M);
}

fn severity_color(pct: f64) -> Color32 {
    if pct >= 85.0 {
        Color32::from_rgb(110, 190, 120)
    } else if pct >= 70.0 {
        Color32::from_rgb(220, 190, 90)
    } else {
        Color32::from_rgb(220, 140, 80)
    }
}

fn is_zerg_race(race: &str) -> bool {
    race.starts_with('Z') || race.starts_with('z')
}
