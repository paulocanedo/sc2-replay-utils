//! Painel lateral — stats de um jogador no instante de scrubbing.
//!
//! Organizado em blocos separados por dividers hairline:
//! 1. Identidade — nome + chip da raça.
//! 2. Supply — barra de progresso com destaque quando blocked.
//! 3. Economia — minerals/gas (com ícones desenhados) + rates,
//!    workers com barra de capacidade.
//! 4. Army — valor total + split mineral/gas + pips de attack/armor.
//! 5. Pesquisas — chips com upgrades pontuais concluídos (não-leveled).
//! 6. Eficiência — building focus, idle time e supply block count.
//!
//! Unidades e estruturas vivas são renderizadas em colunas verticais no
//! painel central (ver `unit_column.rs`), não aqui.

use egui::{
    epaint::Shape, pos2, vec2, Align, Color32, Layout, ProgressBar, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui,
};

use crate::build_order::{format_time, BuildOrderEntry};
use crate::colors::{player_slot_color_bright, ACCENT_WARNING, LABEL_DIM, LABEL_SOFT};
use crate::config::AppConfig;
use crate::locale::{localize, tf, Language};
use crate::production_gap::{compute_idle_periods, compute_idle_periods_ranges, is_zerg_race};
use crate::replay::{PlayerTimeline, StatsSnapshot};
use crate::replay_state::LoadedReplay;
use crate::supply_block::SupplyBlockEntry;
use crate::tokens::{size_body, size_caption, size_subtitle, SPACE_S, SPACE_XS};
use crate::widgets::{chip, player_identity, NameDensity};

use super::entities::structure_attention_at;

/// Cor do diamante de minerals. Azul-claro próximo do cristal in-game.
const MINERAL_COLOR: Color32 = Color32::from_rgb(100, 180, 230);
/// Cor do círculo de gas. Verde-claro próximo do geyser Vespene.
const GAS_COLOR: Color32 = Color32::from_rgb(90, 200, 150);
/// Workers por base em saturação ideal (≈ 3 por patch + 3 por geyser).
const WORKERS_PER_BASE_IDEAL: i32 = 22;
/// Teto do denominador de saturação. Acima disso, `👷 N/M` repete o
/// numerador (barra cheia) pra não exibir razões tipo `90/80` que
/// saturam visualmente mas não dizem nada acionável.
const WORKER_SATURATION_CAP: i32 = 80;

/// Renderiza o painel lateral de um jogador. Faz lookup de todos os
/// dados derivados (`production`, `supply_blocks`) diretamente no
/// `LoadedReplay` pra evitar um fan-out de argumentos.
pub(super) fn player_side_panel(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    idx: usize,
    game_loop: u32,
    cfg: &AppConfig,
) {
    let Some(p) = loaded.timeline.players.get(idx) else {
        return;
    };
    let supply_blocks: &[SupplyBlockEntry] = loaded
        .supply_blocks_per_player
        .get(idx)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let loops_per_second = loaded.timeline.loops_per_second;
    let lang = cfg.language;
    let slot_color = player_slot_color_bright(idx);

    ui.add_space(SPACE_S);
    header(ui, p, idx, cfg, lang);
    ui.add_space(SPACE_XS);
    ui.separator();

    match p.stats_at(game_loop) {
        Some(s) => {
            ui.add_space(SPACE_XS);
            supply_bar(ui, s, slot_color, lang);
            ui.add_space(SPACE_S);
            economy_block(ui, p, s, game_loop, cfg, lang);
            ui.add_space(SPACE_S);
            ui.separator();
            ui.add_space(SPACE_XS);
            army_block(ui, p, s, game_loop, slot_color, cfg, lang);
        }
        None => {
            ui.add_space(SPACE_S);
            ui.weak("—");
        }
    }

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    let research_entries: &[BuildOrderEntry] = loaded
        .build_order
        .as_ref()
        .and_then(|bo| bo.players.get(idx))
        .map(|pbo| pbo.entries.as_slice())
        .unwrap_or(&[]);
    researches_block(ui, research_entries, game_loop, loops_per_second, lang);

    ui.add_space(SPACE_XS);
    ui.separator();
    ui.add_space(SPACE_XS);
    efficiency_block(ui, p, game_loop, supply_blocks, loops_per_second, lang);
}

// ── Header ─────────────────────────────────────────────────────────────

fn header(ui: &mut Ui, p: &PlayerTimeline, idx: usize, cfg: &AppConfig, lang: Language) {
    let is_user = cfg.is_user(&p.name);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        player_identity(
            ui,
            &p.name,
            &p.race,
            idx,
            is_user,
            NameDensity::Normal,
            cfg,
            lang,
        );
    });
}

// ── Supply bar ─────────────────────────────────────────────────────────

fn supply_bar(ui: &mut Ui, s: &StatsSnapshot, slot_color: Color32, lang: Language) {
    let cap = s.supply_made.min(200);
    let frac = if cap > 0 {
        (s.supply_used as f32 / cap as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let blocked = s.supply_made > 0 && s.supply_used >= s.supply_made;
    let bar_color = if blocked { ACCENT_WARNING } else { slot_color };
    let label = if blocked {
        format!("⚠ {}/{}", s.supply_used, cap)
    } else {
        format!("{}/{}", s.supply_used, cap)
    };
    let tt_key = if blocked {
        "timeline.tt.supply_blocked"
    } else {
        "timeline.tt.supply"
    };
    ui.add(
        ProgressBar::new(frac)
            .fill(bar_color)
            .text(RichText::new(label).small().strong()),
    )
    .on_hover_text(tf(tt_key, lang, &[]));
}

// ── Economy block ──────────────────────────────────────────────────────

fn economy_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    s: &StatsSnapshot,
    game_loop: u32,
    cfg: &AppConfig,
    lang: Language,
) {
    resource_row(ui, MINERAL_COLOR, s.minerals, s.minerals_rate, true, cfg, lang);
    resource_row(ui, GAS_COLOR, s.vespene, s.vespene_rate, false, cfg, lang);
    worker_row(ui, p, s, game_loop, lang);
}

fn resource_row(
    ui: &mut Ui,
    icon_color: Color32,
    value: i32,
    rate: i32,
    is_mineral: bool,
    cfg: &AppConfig,
    lang: Language,
) {
    let tt_key = if is_mineral {
        "timeline.tt.minerals"
    } else {
        "timeline.tt.vespene"
    };
    ui.horizontal(|ui| {
        let size = size_body(cfg);
        let (resp, painter) = ui.allocate_painter(vec2(size, size), Sense::hover());
        if is_mineral {
            paint_mineral_icon(&painter, resp.rect, icon_color);
        } else {
            paint_gas_icon(&painter, resp.rect, icon_color);
        }
        ui.monospace(value.to_string());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("+{}/m", rate))
                    .size(size_caption(cfg))
                    .color(LABEL_DIM),
            );
        });
    })
    .response
    .on_hover_text(tf(tt_key, lang, &[]));
}

fn worker_row(ui: &mut Ui, p: &PlayerTimeline, s: &StatsSnapshot, game_loop: u32, lang: Language) {
    // Mostra só o número de workers; a razão workers/saturação fica
    // implícita na mini-barra. `worker_capacity_at` × `WORKERS_PER_BASE_IDEAL`
    // dá a saturação alvo — a barra satura em 100% assim que o jogador
    // ultrapassa o cap, mantendo o indicador legível em lategame.
    let bases = p.worker_capacity_at(game_loop);
    let saturation = (bases * WORKERS_PER_BASE_IDEAL).min(WORKER_SATURATION_CAP);
    let frac = if s.workers >= WORKER_SATURATION_CAP || saturation == 0 {
        1.0
    } else {
        (s.workers as f32 / saturation as f32).clamp(0.0, 1.0)
    };
    ui.horizontal(|ui| {
        ui.monospace(format!("👷 {}", s.workers));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            draw_mini_bar(ui, 42.0, 4.0, frac, LABEL_SOFT);
        });
    })
    .response
    .on_hover_text(tf("timeline.tt.workers", lang, &[]));
}

// ── Army block ─────────────────────────────────────────────────────────

fn army_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    s: &StatsSnapshot,
    game_loop: u32,
    slot_color: Color32,
    cfg: &AppConfig,
    lang: Language,
) {
    let total = s.army_value_minerals + s.army_value_vespene;
    ui.label(
        RichText::new(format!("⚔ {total}"))
            .size(size_subtitle(cfg))
            .strong()
            .color(slot_color),
    )
    .on_hover_text(tf("timeline.tt.army_value", lang, &[]));
    let caption = size_caption(cfg);
    ui.horizontal(|ui| {
        let (resp, painter) = ui.allocate_painter(vec2(caption, caption), Sense::hover());
        paint_mineral_icon(&painter, resp.rect, MINERAL_COLOR);
        ui.label(
            RichText::new(s.army_value_minerals.to_string())
                .size(caption)
                .color(LABEL_DIM),
        );
        ui.label(RichText::new("·").size(caption).color(LABEL_DIM));
        let (resp, painter) = ui.allocate_painter(vec2(caption, caption), Sense::hover());
        paint_gas_icon(&painter, resp.rect, GAS_COLOR);
        ui.label(
            RichText::new(s.army_value_vespene.to_string())
                .size(caption)
                .color(LABEL_DIM),
        );
    })
    .response
    .on_hover_text(tf("timeline.tt.army_split", lang, &[]));

    let atk = p.attack_level_at(game_loop);
    let arm = p.armor_level_at(game_loop);
    if atk > 0 || arm > 0 {
        ui.add_space(SPACE_XS);
        ui.horizontal(|ui| {
            if atk > 0 {
                chip(ui, &format!("⚔+{atk}"), true, Some(slot_color))
                    .on_hover_text(tf("timeline.tt.atk_upgrade", lang, &[]));
            }
            if arm > 0 {
                chip(ui, &format!("🛡+{arm}"), true, Some(slot_color))
                    .on_hover_text(tf("timeline.tt.arm_upgrade", lang, &[]));
            }
        });
    }
}

// ── Researches block ───────────────────────────────────────────────────
//
// Lista os upgrades pontuais até o instante corrente — Stim, WarpGate,
// Blink, etc. Fonte é o `BuildOrderResult` (não a stream crua de
// `UpgradeEntry`), porque ali já temos start_loop reconciliado via cmd
// matching além do finish_loop. Inclui também pesquisas em andamento
// (start ≤ now < finish) — o fim é o `finish_loop` projetado e a linha
// é renderizada com cor mais apagada + sufixo `…` pra sinalizar que é
// estimativa.
//
// Os upgrades com níveis (`*Level1/2/3`) não aparecem aqui porque já
// estão representados como pips `⚔+N` / `🛡+N` no army block; duplicar
// seria ruído. A ordenação é cronológica — mais antigo no topo. Layout
// é uma pesquisa por linha com slot de ícone reservado à esquerda
// (placeholder por enquanto; assets entram depois).

/// Gap horizontal entre o slot do ícone e o nome da pesquisa.
const RESEARCH_ICON_GAP: f32 = 6.0;

/// Lado do slot de ícone (futuro). Pareia com a altura de Body × 1.4
/// pra ficar próximo do tamanho dos cards de unidade/estrutura sem
/// depender de `card_size` cross-module.
fn research_icon_side(ui: &Ui) -> f32 {
    (ui.text_style_height(&egui::TextStyle::Body) * 1.4).round()
}

fn researches_block(
    ui: &mut Ui,
    entries: &[BuildOrderEntry],
    game_loop: u32,
    loops_per_second: f64,
    lang: Language,
) {
    // `Reward*` são achievements cosméticos (portrait, spray, voice set)
    // que entram na stream de upgrades mas não têm efeito de jogo.
    // `game_loop == 0` pega buffs de bootstrap aplicados antes do jogo
    // começar — não representam decisões de tech.
    let visible: Vec<&BuildOrderEntry> = entries
        .iter()
        .filter(|e| {
            e.is_upgrade
                && e.game_loop > 0
                && e.game_loop <= game_loop
                && !is_level_upgrade_name(&e.action)
                && !e.action.starts_with("Reward")
        })
        .collect();
    if visible.is_empty() {
        ui.label(
            RichText::new(tf("timeline.stats.researches_none", lang, &[]))
                .small()
                .color(LABEL_DIM),
        );
        return;
    }
    ui.spacing_mut().item_spacing.y = SPACE_XS;
    for e in visible {
        let full = localize(&e.action, lang);
        let start = format_time(e.game_loop, loops_per_second);
        let end = format_time(e.finish_loop, loops_per_second);
        let in_progress = e.finish_loop > game_loop;
        let times = if in_progress {
            format!("{start} → {end}…")
        } else {
            format!("{start} → {end}")
        };
        let times_color = if in_progress { LABEL_DIM } else { LABEL_SOFT };

        let start_secs = (e.game_loop as f64 / loops_per_second).round() as u32;
        let end_secs = (e.finish_loop as f64 / loops_per_second).round() as u32;
        let tooltip = tf(
            if in_progress {
                "timeline.tt.research_chip_in_progress"
            } else {
                "timeline.tt.research_chip"
            },
            lang,
            &[
                ("name", full),
                ("start_mm", &format!("{:02}", start_secs / 60)),
                ("start_ss", &format!("{:02}", start_secs % 60)),
                ("end_mm", &format!("{:02}", end_secs / 60)),
                ("end_ss", &format!("{:02}", end_secs % 60)),
            ],
        );

        let response = ui
            .horizontal(|ui| {
                let side = research_icon_side(ui);
                let (icon_rect, _) =
                    ui.allocate_exact_size(vec2(side, side), Sense::hover());
                // Placeholder do ícone: borda fina sem fill. Quando os
                // assets entrarem, troca por `egui::Image::new(icon)
                // .fit_to_exact_size(...).paint_at(ui, icon_rect)`.
                ui.painter().rect_stroke(
                    icon_rect,
                    2.0,
                    Stroke::new(1.0, LABEL_DIM),
                    StrokeKind::Inside,
                );
                ui.add_space(RESEARCH_ICON_GAP);
                ui.label(RichText::new(full).small().color(LABEL_SOFT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(times).small().color(times_color));
                });
            })
            .response;
        response.on_hover_text(tooltip);
    }
}

/// Espelha `build_order::classify::is_leveled_upgrade` — precisamos do
/// mesmo critério para filtrar os upgrades que já aparecem como pips
/// attack/armor no army block. Duplicar a heurística aqui evita expor
/// API cross-module só pra três linhas.
fn is_level_upgrade_name(name: &str) -> bool {
    name.ends_with("Level1") || name.ends_with("Level2") || name.ends_with("Level3")
}

// ── Efficiency block ───────────────────────────────────────────────────

fn efficiency_block(
    ui: &mut Ui,
    p: &PlayerTimeline,
    game_loop: u32,
    supply_blocks: &[SupplyBlockEntry],
    loops_per_second: f64,
    lang: Language,
) {
    // Building focus — retained metric with inline mini-bar.
    let (att, tot) = structure_attention_at(p, game_loop);
    let bldg_tt_key = if tot == 0 {
        "timeline.tt.bldg_focus_none"
    } else {
        "timeline.tt.bldg_focus"
    };
    ui.horizontal(|ui| {
        if tot == 0 {
            ui.label(
                RichText::new(tf("timeline.stats.bldg_focus_none", lang, &[]))
                    .small()
                    .color(LABEL_DIM),
            );
        } else {
            let pct = att as f32 * 100.0 / tot as f32;
            ui.label(
                RichText::new(format!("🏢 {pct:.0}%"))
                    .small()
                    .color(LABEL_SOFT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                draw_mini_bar(ui, 32.0, 3.0, pct / 100.0, LABEL_DIM);
            });
        }
    })
    .response
    .on_hover_text(tf(bldg_tt_key, lang, &[]));

    // Worker idle — T/P apenas; Zerg usa larva model (sem CC/Nexus cap).
    if !is_zerg_race(&p.race) {
        let (_, worker_idle, _) =
            compute_idle_periods(&p.worker_births, &p.worker_capacity, game_loop);
        if worker_idle > 0 {
            let secs = (worker_idle as f64 / loops_per_second).round() as u32;
            ui.label(
                RichText::new(tf(
                    "timeline.stats.idle_worker",
                    lang,
                    &[("secs", &secs.to_string())],
                ))
                .small()
                .color(LABEL_DIM),
            )
            .on_hover_text(tf("timeline.tt.idle_worker", lang, &[]));
        }
    }

    // Army idle — todas as raças (Zerg usa slots de Hatchery/Lair/Hive).
    let (_, army_idle, _) =
        compute_idle_periods_ranges(&p.army_productions, &p.army_capacity, game_loop);
    if army_idle > 0 {
        let secs = (army_idle as f64 / loops_per_second).round() as u32;
        ui.label(
            RichText::new(tf(
                "timeline.stats.idle_army",
                lang,
                &[("secs", &secs.to_string())],
            ))
            .small()
            .color(LABEL_DIM),
        )
        .on_hover_text(tf("timeline.tt.idle_army", lang, &[]));
    }

    // Supply block — acumulado até game_loop (contagem e tempo).
    let (count, total_loops) =
        supply_blocks
            .iter()
            .filter(|b| b.start_loop < game_loop)
            .fold((0u32, 0u32), |(c, t), b| {
                let end = b.end_loop.min(game_loop);
                (c + 1, t + end.saturating_sub(b.start_loop))
            });
    if count > 0 {
        let secs = (total_loops as f64 / loops_per_second).round() as u32;
        ui.label(
            RichText::new(tf(
                "timeline.stats.blocks",
                lang,
                &[("count", &count.to_string()), ("secs", &secs.to_string())],
            ))
            .small()
            .color(ACCENT_WARNING),
        )
        .on_hover_text(tf("timeline.tt.blocks", lang, &[]));
    }
}

// ── Drawing primitives ─────────────────────────────────────────────────

fn paint_mineral_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.4;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x, c.y - r),
            pos2(c.x + r * 0.75, c.y),
            pos2(c.x, c.y + r),
            pos2(c.x - r * 0.75, c.y),
        ],
        color,
        Stroke::NONE,
    ));
}

fn paint_gas_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.32;
    painter.circle_filled(c, r, color);
}

/// Barra fina usada como inline indicator (worker cap, building focus).
/// Track em cinza escuro, fill em `color`.
fn draw_mini_bar(ui: &mut Ui, width: f32, height: f32, frac: f32, color: Color32) {
    let (resp, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 1.0, Color32::from_gray(50));
    let fill_w = rect.width() * frac.clamp(0.0, 1.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 1.0, color);
    }
}
