// Card de insight: turning point da partida.
//
// Identifica o instante onde a vantagem de army_value do POV colapsou
// — o peak do `pov - opp` e, depois dele, o primeiro loop em que o
// diff fica negativo (adversário ultrapassou). Oferece um botão
// "Open in Timeline" que faz seek: retorna o target_loop pro caller
// que redireciona e posiciona o slider da aba Timeline.

use egui::{RichText, Ui};

use crate::army_value::ArmySnapshot;
use crate::config::AppConfig;
use crate::locale::{t, tf};
use crate::replay_state::{loop_to_secs, LoadedReplay};
use crate::tokens::{size_subtitle, SPACE_M, SPACE_S};

use super::card::insight_card;

/// Valor combinado mínimo (pov + opp) pra um sample ser considerado
/// na análise. Amostras com army_value pequeno gerariam ratios voláteis
/// no early game (primeiros units stoppers etc.).
const MIN_COMBINED_VALUE: i32 = 500;

/// Retorna `Some(target_loop)` quando o usuário clicou no botão
/// "Open in Timeline" — caller deve aplicar seek + trocar de tab.
pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    player_idx: usize,
) -> Option<u32> {
    let lang = config.language;

    // 1v1 apenas — turning point só faz sentido com um adversário.
    if loaded.timeline.players.len() != 2 {
        return None;
    }
    let Some(av) = loaded.army.as_ref() else {
        return None;
    };
    let pov = av.players.get(player_idx)?;
    let opp_idx = if player_idx == 0 { 1 } else { 0 };
    let opp = av.players.get(opp_idx)?;

    let outcome = detect_turning_point(&pov.snapshots, &opp.snapshots);

    let title = t("insight.turning_point.title", lang).to_string();
    let help_text = t("insight.turning_point.help", lang).to_string();

    let mut seek: Option<u32> = None;
    insight_card(ui, config, "turning_point", &title, &help_text, |ui| {
        seek = render_body(ui, config, &outcome, av.loops_per_second);
    });
    seek
}

enum Outcome {
    /// POV nunca esteve na frente — perdeu ou estava atrás o tempo todo.
    NeverAhead,
    /// POV esteve na frente em algum momento e foi ultrapassado.
    Overtaken {
        peak_loop: u32,
        peak_diff: i32,
        turning_loop: u32,
    },
    /// POV esteve na frente e terminou ainda na frente — não houve
    /// turning point pela métrica de army_value (o jogo pode ter
    /// sido decidido por outros fatores).
    HeldAdvantage { peak_diff: i32 },
    /// Não há dados suficientes (poucos samples com valor relevante).
    InsufficientData,
}

fn detect_turning_point(pov: &[ArmySnapshot], opp: &[ArmySnapshot]) -> Outcome {
    // Assume que os dois vetores compartilham a mesma timebase dos
    // `stats` (o parser emite um snapshot por player por tick), então
    // o pareamento posicional é válido. Trunca ao menor pra robustez.
    let n = pov.len().min(opp.len());
    if n < 3 {
        return Outcome::InsufficientData;
    }

    let mut peak_diff: i32 = i32::MIN;
    let mut peak_loop: u32 = 0;
    let mut any_considered = false;
    for i in 0..n {
        let p = &pov[i];
        let o = &opp[i];
        if p.army_total + o.army_total < MIN_COMBINED_VALUE {
            continue;
        }
        any_considered = true;
        let diff = p.army_total - o.army_total;
        if diff > peak_diff {
            peak_diff = diff;
            peak_loop = p.game_loop;
        }
    }
    if !any_considered {
        return Outcome::InsufficientData;
    }
    if peak_diff <= 0 {
        return Outcome::NeverAhead;
    }

    // Varre a partir do peak pra encontrar o primeiro loop em que
    // diff < 0 (adversário ultrapassou).
    let mut passed_peak = false;
    for i in 0..n {
        let p = &pov[i];
        let o = &opp[i];
        if !passed_peak {
            if p.game_loop >= peak_loop {
                passed_peak = true;
            }
            continue;
        }
        if p.army_total + o.army_total < MIN_COMBINED_VALUE {
            continue;
        }
        if p.army_total < o.army_total {
            return Outcome::Overtaken {
                peak_loop,
                peak_diff,
                turning_loop: p.game_loop,
            };
        }
    }
    Outcome::HeldAdvantage { peak_diff }
}

fn render_body(
    ui: &mut Ui,
    config: &AppConfig,
    outcome: &Outcome,
    lps: f64,
) -> Option<u32> {
    let lang = config.language;
    let size = size_subtitle(config);
    let mut seek: Option<u32> = None;

    match outcome {
        Outcome::InsufficientData => {
            ui.label(
                RichText::new(t("insight.turning_point.insufficient_data", lang)).italics(),
            );
        }
        Outcome::NeverAhead => {
            ui.label(
                RichText::new(t("insight.turning_point.never_ahead", lang)).italics(),
            );
        }
        Outcome::HeldAdvantage { peak_diff } => {
            ui.label(
                RichText::new(tf(
                    "insight.turning_point.held_advantage",
                    lang,
                    &[("peak", &peak_diff.to_string())],
                ))
                .italics(),
            );
        }
        Outcome::Overtaken {
            peak_loop,
            peak_diff,
            turning_loop,
        } => {
            let peak_secs = loop_to_secs(*peak_loop, lps) as u32;
            let turn_secs = loop_to_secs(*turning_loop, lps) as u32;
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(t("insight.turning_point.turned_at", lang)).size(size * 0.85),
                );
                ui.label(
                    RichText::new(format!("{}:{:02}", turn_secs / 60, turn_secs % 60))
                        .size(size * 1.4)
                        .strong(),
                );
            });
            ui.add_space(SPACE_S);
            ui.label(
                RichText::new(tf(
                    "insight.turning_point.context_line",
                    lang,
                    &[
                        ("peak_mm", &(peak_secs / 60).to_string()),
                        ("peak_ss", &format!("{:02}", peak_secs % 60)),
                        ("peak_diff", &peak_diff.to_string()),
                    ],
                ))
                .italics(),
            );
            ui.add_space(SPACE_S);
            if ui
                .button(t("insight.turning_point.open_in_timeline", lang))
                .clicked()
            {
                seek = Some(*turning_loop);
            }
        }
    }

    ui.add_space(SPACE_M);
    seek
}
