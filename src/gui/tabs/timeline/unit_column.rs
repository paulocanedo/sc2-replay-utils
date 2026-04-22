//! Coluna vertical de unidades + estruturas vivas, renderizada dentro do
//! `CentralPanel` da Timeline (uma por jogador, flanqueando o minimap).
//!
//! Contém também os helpers compartilhados com o side panel: lookup de
//! sprite (`unit_icon`), abreviações textuais (`unit_abbrev`,
//! `structure_abbrev`), colapso de variantes de estrutura
//! (`structure_canonical`), e o chip com ícone parametrizado
//! (`icon_chip`).

use std::collections::HashMap;

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, Ui};

use crate::colors::{player_slot_color, player_slot_color_bright};
use crate::locale::{localize, tf, Language};
use crate::replay::PlayerTimeline;
use crate::tokens::SPACE_XS;

use super::entities::{collect_alive_structures, collect_alive_units};

/// Alpha do background semi-transparente pintado com a cor do jogador.
/// ~20% de opacidade — visível o suficiente pra identificar o dono do
/// conteúdo mas sem lavar os ícones renderizados por cima.
const SECTION_BG_ALPHA: u8 = 50;

/// Raio dos cantos do container de seção e da borda de hover dos cards.
const CARD_CORNER_RADIUS: f32 = 4.0;

/// Padding interno do container de seção — folga entre o background e
/// a primeira/última linha de cards.
const SECTION_INNER_PAD: i8 = 6;

/// Fill semi-transparente na cor do slot. Usado como background único
/// do container que envolve todos os cards de uma seção (unidades ou
/// estruturas). Cards individuais não recebem background próprio —
/// fica um bloco visual coeso em vez de uma lista de pílulas.
fn section_bg(slot_idx: usize) -> Color32 {
    let c = player_slot_color(slot_idx);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), SECTION_BG_ALPHA)
}

/// Frame pintado com o fill da seção + cantos arredondados + padding
/// interno. Usado nas duas seções (`render_units_section` e
/// `render_structures_section`) pra envolver o stack vertical de cards
/// num único bloco colorido.
fn section_frame(slot_idx: usize) -> egui::Frame {
    egui::Frame::new()
        .fill(section_bg(slot_idx))
        .corner_radius(CARD_CORNER_RADIUS)
        .inner_margin(egui::Margin::same(SECTION_INNER_PAD))
}

/// Gap horizontal entre o ícone e o número de contagem dentro do card.
const CARD_ICON_TEXT_GAP: f32 = 4.0;

/// Número de glifos reservados para a contagem. 3 cobre late game
/// (centenas de Zerglings) sem deixar o card variar de largura.
const CARD_COUNT_GLYPHS: f32 = 3.0;

/// Dimensões do card `[ícone][count]`. Sem background e com largura
/// travada (ícone + gap + slot de 3 dígitos), pra que todos os cards
/// da coluna alinhem verticalmente independente da contagem. A altura
/// é o lado do ícone — sem padding extra.
fn card_size(ui: &Ui, size_factor: f32) -> (f32, egui::Vec2) {
    let icon_side = (ui.text_style_height(&egui::TextStyle::Body) * size_factor).round();
    let font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let glyph_w = ui
        .painter()
        .layout_no_wrap("8".to_string(), font_id, Color32::WHITE)
        .rect
        .width();
    let width = icon_side + CARD_ICON_TEXT_GAP + glyph_w * CARD_COUNT_GLYPHS;
    (icon_side, egui::vec2(width, icon_side))
}

/// Pinta apenas a borda fina na cor do slot quando o card está
/// hovered. Sem fill — o background fica no container da seção.
fn paint_hover_ring(ui: &Ui, rect: egui::Rect, slot_idx: usize) {
    ui.painter().rect_stroke(
        rect,
        CARD_CORNER_RADIUS,
        Stroke::new(1.0, player_slot_color_bright(slot_idx)),
        StrokeKind::Inside,
    );
}

/// Card compacto `[ícone] [contagem]`. Sem background próprio — o
/// container da seção pinta o fill único que envolve todos os cards.
/// Tamanho fixo (ícone + gap + slot de 3 dígitos) pra que cards alinhem
/// verticalmente independente do número de dígitos. Mostra ring de
/// hover na cor do slot pra feedback de interação.
pub(super) fn icon_chip(
    ui: &mut Ui,
    icon: egui::ImageSource<'static>,
    count: i32,
    size_factor: f32,
    slot_idx: usize,
) -> egui::Response {
    let (icon_side, size) = card_size(ui, size_factor);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let icon_rect = egui::Rect::from_min_size(rect.min, egui::vec2(icon_side, icon_side));
        egui::Image::new(icon)
            .fit_to_exact_size(egui::vec2(icon_side, icon_side))
            .paint_at(ui, icon_rect);
        let font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
        ui.painter().text(
            egui::pos2(icon_rect.right() + CARD_ICON_TEXT_GAP, rect.center().y),
            egui::Align2::LEFT_CENTER,
            count.to_string(),
            font_id,
            Color32::from_gray(220),
        );
        if response.hovered() {
            paint_hover_ring(ui, rect, slot_idx);
        }
    }
    response
}

/// Variante do card compacto onde o slot do ícone é ocupado por uma
/// abreviação textual (3 letras). Usado para unidades sem sprite.
/// Mantém o mesmo tamanho fixo de `icon_chip` pra preservar o
/// alinhamento vertical da coluna.
fn text_chip(
    ui: &mut Ui,
    abbrev: &str,
    count: i32,
    size_factor: f32,
    slot_idx: usize,
) -> egui::Response {
    let (icon_side, size) = card_size(ui, size_factor);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
        let icon_rect = egui::Rect::from_min_size(rect.min, egui::vec2(icon_side, icon_side));
        ui.painter().text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            abbrev,
            font_id.clone(),
            Color32::from_gray(220),
        );
        ui.painter().text(
            egui::pos2(icon_rect.right() + CARD_ICON_TEXT_GAP, rect.center().y),
            egui::Align2::LEFT_CENTER,
            count.to_string(),
            font_id,
            Color32::from_gray(220),
        );
        if response.hovered() {
            paint_hover_ring(ui, rect, slot_idx);
        }
    }
    response
}

/// Renderiza a coluna vertical com unidades + separador + estruturas do
/// jogador. Scroll automático pra acomodar comps com muitos tipos.
pub(super) fn render_player_column(
    ui: &mut Ui,
    p: &PlayerTimeline,
    slot_idx: usize,
    game_loop: u32,
    lang: Language,
    icon_size_factor: f32,
) {
    egui::ScrollArea::vertical()
        .id_salt(("timeline_units_col", p.name.as_str()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            render_units_section(ui, p, slot_idx, game_loop, lang, icon_size_factor);
            ui.add_space(SPACE_XS);
            ui.separator();
            ui.add_space(SPACE_XS);
            render_structures_section(ui, p, slot_idx, game_loop, lang, icon_size_factor);
        });
}

fn render_units_section(
    ui: &mut Ui,
    p: &PlayerTimeline,
    slot_idx: usize,
    game_loop: u32,
    lang: Language,
    size_factor: f32,
) {
    let raw = collect_alive_units(p, game_loop);
    if raw.is_empty() {
        ui.label(
            RichText::new(tf("timeline.stats.units_none", lang, &[]))
                .small()
                .color(crate::colors::LABEL_DIM),
        );
        return;
    }
    // Agrega variantes de estado (Sieged/Burrowed/Cocoon/Phased/AG/AP…)
    // no nome canônico antes de renderizar — caso contrário
    // `SiegeTank` e `SiegeTankSieged` apareceriam como dois chips
    // distintos mesmo compartilhando sprite e abreviação. Entidades
    // desconhecidas (mods/patches novos) caem no `else` e preservam o
    // nome bruto.
    let mut agg: HashMap<String, i32> = HashMap::new();
    for (ty, count) in &raw {
        let canonical = unit_canonical(ty);
        let key = if canonical.is_empty() {
            ty.clone()
        } else {
            canonical.to_string()
        };
        *agg.entry(key).or_insert(0) += count;
    }
    let mut entries: Vec<(String, i32)> = agg.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    section_frame(slot_idx).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = SPACE_XS;
        for (canonical, count) in entries {
            let tooltip = tf(
                "timeline.tt.unit_chip",
                lang,
                &[
                    ("name", localize(&canonical, lang)),
                    ("count", &count.to_string()),
                ],
            );
            if let Some(icon) = unit_icon(&canonical) {
                icon_chip(ui, icon, count, size_factor, slot_idx).on_hover_text(tooltip);
            } else {
                text_chip(ui, &unit_abbrev(&canonical), count, size_factor, slot_idx)
                    .on_hover_text(tooltip);
            }
        }
    });
}

fn render_structures_section(
    ui: &mut Ui,
    p: &PlayerTimeline,
    slot_idx: usize,
    game_loop: u32,
    lang: Language,
    size_factor: f32,
) {
    let raw = collect_alive_structures(p, game_loop);
    let mut agg: HashMap<&'static str, i32> = HashMap::new();
    for (ty, count) in &raw {
        let canonical = structure_canonical(ty);
        if canonical.is_empty() {
            continue;
        }
        *agg.entry(canonical).or_insert(0) += count;
    }
    if agg.is_empty() {
        ui.label(
            RichText::new(tf("timeline.stats.structures_none", lang, &[]))
                .small()
                .color(crate::colors::LABEL_DIM),
        );
        return;
    }
    let mut entries: Vec<(&'static str, i32)> = agg.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    section_frame(slot_idx).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = SPACE_XS;
        for (canonical, count) in entries {
            let tooltip = tf(
                "timeline.tt.unit_chip",
                lang,
                &[
                    ("name", localize(canonical, lang)),
                    ("count", &count.to_string()),
                ],
            );
            if let Some(icon) = structure_icon(canonical) {
                icon_chip(ui, icon, count, size_factor, slot_idx).on_hover_text(tooltip);
            } else {
                text_chip(ui, structure_abbrev(canonical), count, size_factor, slot_idx)
                    .on_hover_text(tooltip);
            }
        }
    });
}

/// Colapsa variantes de estado (Sieged, Burrowed, Cocoon, Phased, AG,
/// AP, PhaseShift, SiegeMode, Transport…) no nome canônico da unidade.
/// Análogo ao `structure_canonical` — usado pela agregação em
/// `render_units_section` pra garantir que `SiegeTank` e
/// `SiegeTankSieged` virem um único chip com a soma das contagens.
/// Retorna string vazia pra entidades desconhecidas — caller preserva
/// o nome bruto nesse caso.
pub(super) fn unit_canonical(name: &str) -> &'static str {
    match name {
        // Terran
        "SCV" => "SCV",
        "MULE" => "MULE",
        "Marine" => "Marine",
        "Marauder" => "Marauder",
        "Reaper" => "Reaper",
        "Ghost" | "GhostAlternate" => "Ghost",
        "Hellion" => "Hellion",
        "HellionTank" => "HellionTank",
        "SiegeTank" | "SiegeTankSieged" => "SiegeTank",
        "Cyclone" => "Cyclone",
        "WidowMine" | "WidowMineBurrowed" => "WidowMine",
        "Thor" | "ThorAP" => "Thor",
        "VikingFighter" | "VikingAssault" => "VikingFighter",
        "Medivac" => "Medivac",
        "Liberator" | "LiberatorAG" => "Liberator",
        "Raven" => "Raven",
        "Banshee" => "Banshee",
        "Battlecruiser" => "Battlecruiser",
        // Protoss
        "Probe" => "Probe",
        "Zealot" => "Zealot",
        "Stalker" => "Stalker",
        "Sentry" => "Sentry",
        "Adept" | "AdeptPhaseShift" => "Adept",
        "HighTemplar" => "HighTemplar",
        "DarkTemplar" => "DarkTemplar",
        "Archon" => "Archon",
        "Immortal" => "Immortal",
        "Colossus" => "Colossus",
        "Disruptor" | "DisruptorPhased" => "Disruptor",
        "Observer" | "ObserverSiegeMode" => "Observer",
        "WarpPrism" | "WarpPrismPhasing" => "WarpPrism",
        "Phoenix" => "Phoenix",
        "VoidRay" => "VoidRay",
        "Oracle" => "Oracle",
        "Tempest" => "Tempest",
        "Carrier" => "Carrier",
        "Mothership" => "Mothership",
        // Zerg
        "Drone" => "Drone",
        "Queen" => "Queen",
        "Zergling" => "Zergling",
        "Baneling" | "BanelingCocoon" => "Baneling",
        "Roach" | "RoachBurrowed" => "Roach",
        "Ravager" | "RavagerCocoon" => "Ravager",
        "Hydralisk" | "HydraliskBurrowed" => "Hydralisk",
        "LurkerMP" | "LurkerMPBurrowed" | "LurkerMPEgg" => "LurkerMP",
        "Mutalisk" => "Mutalisk",
        "Corruptor" => "Corruptor",
        "BroodLord" | "BroodLordCocoon" => "BroodLord",
        "Infestor" | "InfestorBurrowed" => "Infestor",
        "SwarmHostMP" | "SwarmHostBurrowedMP" => "SwarmHostMP",
        "Viper" => "Viper",
        "Ultralisk" | "UltraliskBurrowed" => "Ultralisk",
        "Overlord" | "OverlordTransport" => "Overlord",
        "Overseer" | "OverseerSiegeMode" => "Overseer",
        // Neutrals / shared spawns
        "Larva" => "Larva",
        "Interceptor" => "Interceptor",
        "AutoTurret" => "AutoTurret",
        "Locust" | "LocustMP" | "LocustMPFlying" => "LocustMP",
        "Broodling" => "Broodling",
        "Changeling"
        | "ChangelingMarine"
        | "ChangelingMarineShield"
        | "ChangelingZealot"
        | "ChangelingZergling"
        | "ChangelingZerglingWings" => "Changeling",
        _ => "",
    }
}

/// Mapeia `entity_type` → ícone PNG embutido. Filenames casam exatamente
/// com o nome canônico da unidade (ex.: `Marine.png`, `SiegeTank.png`).
/// Variantes de estado (Sieged, Burrowed, Alternate…) colapsam na forma
/// base — são o mesmo modelo visualmente. Retorna `None` pra entidades
/// raras (Larva, Interceptor, Changeling…) — caller cai no fallback de
/// abreviação.
pub(super) fn unit_icon(entity_type: &str) -> Option<egui::ImageSource<'static>> {
    use egui::include_image;
    let src = match entity_type {
        // Terran
        "SCV" => include_image!("../../../../assets/units/terran/SCV.png"),
        "MULE" => include_image!("../../../../assets/units/terran/MULE.png"),
        "Marine" => include_image!("../../../../assets/units/terran/Marine.png"),
        "Marauder" => include_image!("../../../../assets/units/terran/Marauder.png"),
        "Reaper" => include_image!("../../../../assets/units/terran/Reaper.png"),
        "Ghost" | "GhostAlternate" => {
            include_image!("../../../../assets/units/terran/Ghost.png")
        }
        "Hellion" => include_image!("../../../../assets/units/terran/Hellion.png"),
        "HellionTank" => include_image!("../../../../assets/units/terran/HellionTank.png"),
        "SiegeTank" | "SiegeTankSieged" => {
            include_image!("../../../../assets/units/terran/SiegeTank.png")
        }
        "Cyclone" => include_image!("../../../../assets/units/terran/Cyclone.png"),
        "WidowMine" | "WidowMineBurrowed" => {
            include_image!("../../../../assets/units/terran/WidowMine.png")
        }
        "Thor" | "ThorAP" => include_image!("../../../../assets/units/terran/Thor.png"),
        "VikingFighter" | "VikingAssault" => {
            include_image!("../../../../assets/units/terran/VikingFighter.png")
        }
        "Medivac" => include_image!("../../../../assets/units/terran/Medivac.png"),
        "Liberator" | "LiberatorAG" => {
            include_image!("../../../../assets/units/terran/Liberator.png")
        }
        "Raven" => include_image!("../../../../assets/units/terran/Raven.png"),
        "Banshee" => include_image!("../../../../assets/units/terran/Banshee.png"),
        "Battlecruiser" => include_image!("../../../../assets/units/terran/Battlecruiser.png"),
        // Protoss
        "Probe" => include_image!("../../../../assets/units/protoss/Probe.png"),
        "Zealot" => include_image!("../../../../assets/units/protoss/Zealot.png"),
        "Stalker" => include_image!("../../../../assets/units/protoss/Stalker.png"),
        "Sentry" => include_image!("../../../../assets/units/protoss/Sentry.png"),
        "HighTemplar" => include_image!("../../../../assets/units/protoss/HighTemplar.png"),
        "Adept" | "AdeptPhaseShift" => {
            include_image!("../../../../assets/units/protoss/Adept.png")
        }
        "DarkTemplar" => include_image!("../../../../assets/units/protoss/DarkTemplar.png"),
        "Archon" => include_image!("../../../../assets/units/protoss/Archon.png"),
        "Immortal" => include_image!("../../../../assets/units/protoss/Immortal.png"),
        "Disruptor" | "DisruptorPhased" => {
            include_image!("../../../../assets/units/protoss/Disruptor.png")
        }
        "Colossus" => include_image!("../../../../assets/units/protoss/Colossus.png"),
        "Observer" | "ObserverSiegeMode" => {
            include_image!("../../../../assets/units/protoss/Observer.png")
        }
        "WarpPrism" | "WarpPrismPhasing" => {
            include_image!("../../../../assets/units/protoss/WarpPrism.png")
        }
        "Mothership" => include_image!("../../../../assets/units/protoss/Mothership.png"),
        "Phoenix" => include_image!("../../../../assets/units/protoss/Phoenix.png"),
        "VoidRay" => include_image!("../../../../assets/units/protoss/VoidRay.png"),
        "Oracle" => include_image!("../../../../assets/units/protoss/Oracle.png"),
        "Tempest" => include_image!("../../../../assets/units/protoss/Tempest.png"),
        "Carrier" => include_image!("../../../../assets/units/protoss/Carrier.png"),
        // Zerg
        "Drone" => include_image!("../../../../assets/units/zerg/Drone.png"),
        "Queen" => include_image!("../../../../assets/units/zerg/Queen.png"),
        "Zergling" => include_image!("../../../../assets/units/zerg/Zergling.png"),
        "Baneling" | "BanelingCocoon" => {
            include_image!("../../../../assets/units/zerg/Baneling.png")
        }
        "Roach" | "RoachBurrowed" => include_image!("../../../../assets/units/zerg/Roach.png"),
        "Ravager" | "RavagerCocoon" => {
            include_image!("../../../../assets/units/zerg/Ravager.png")
        }
        "Hydralisk" | "HydraliskBurrowed" => {
            include_image!("../../../../assets/units/zerg/Hydralisk.png")
        }
        "LurkerMP" | "LurkerMPBurrowed" | "LurkerMPEgg" => {
            include_image!("../../../../assets/units/zerg/LurkerMP.png")
        }
        "Mutalisk" => include_image!("../../../../assets/units/zerg/Mutalisk.png"),
        "Corruptor" => include_image!("../../../../assets/units/zerg/Corruptor.png"),
        "BroodLord" | "BroodLordCocoon" => {
            include_image!("../../../../assets/units/zerg/BroodLord.png")
        }
        "Infestor" | "InfestorBurrowed" => {
            include_image!("../../../../assets/units/zerg/Infestor.png")
        }
        "SwarmHostMP" | "SwarmHostBurrowedMP" => {
            include_image!("../../../../assets/units/zerg/SwarmHostMP.png")
        }
        "Viper" => include_image!("../../../../assets/units/zerg/Viper.png"),
        "Ultralisk" | "UltraliskBurrowed" => {
            include_image!("../../../../assets/units/zerg/Ultralisk.png")
        }
        "Overlord" | "OverlordTransport" => {
            include_image!("../../../../assets/units/zerg/Overlord.png")
        }
        "Overseer" | "OverseerSiegeMode" => {
            include_image!("../../../../assets/units/zerg/Overseer.png")
        }
        // Zerg — spawns/ephemerals. Changelings (morphed por Overseer)
        // e Locusts (deploy do Swarm Host) têm variantes de estado que
        // colapsam em um único sprite.
        "Larva" => include_image!("../../../../assets/units/zerg/Larva.png"),
        "Broodling" => include_image!("../../../../assets/units/zerg/Broodling.png"),
        "Egg" => include_image!("../../../../assets/units/zerg/Cocoon.png"),
        "Locust" | "LocustMP" | "LocustMPFlying" => {
            include_image!("../../../../assets/units/zerg/LocustMP.png")
        }
        "Changeling" | "ChangelingMarine" | "ChangelingMarineShield" | "ChangelingZealot"
        | "ChangelingZergling" | "ChangelingZerglingWings" => {
            include_image!("../../../../assets/units/zerg/Changeling.png")
        }
        _ => return None,
    };
    Some(src)
}

/// Abreviação fixa de 3 letras. Fallback textual quando `unit_icon`
/// retorna `None` — unidades raras sem sprite (Larva, Interceptor,
/// Changeling, AutoTurret, Broodling, Locust) caem aqui.
pub(super) fn unit_abbrev(entity_type: &str) -> String {
    match entity_type {
        // Terran
        "SCV" => "SCV",
        "MULE" => "MUL",
        "Marine" => "MAR",
        "Marauder" => "MAU",
        "Reaper" => "REA",
        "Ghost" | "GhostAlternate" => "GHO",
        "Hellion" => "HEL",
        "HellionTank" => "HBT",
        "SiegeTank" | "SiegeTankSieged" => "TNK",
        "Cyclone" => "CYC",
        "WidowMine" | "WidowMineBurrowed" => "WMN",
        "Thor" | "ThorAP" => "THR",
        "VikingFighter" | "VikingAssault" => "VIK",
        "Medivac" => "MDV",
        "Liberator" | "LiberatorAG" => "LIB",
        "Raven" => "RVN",
        "Banshee" => "BSH",
        "Battlecruiser" => "BC",
        // Protoss
        "Probe" => "PRB",
        "Zealot" => "ZEA",
        "Stalker" => "STK",
        "Sentry" => "SEN",
        "Adept" | "AdeptPhaseShift" => "ADP",
        "HighTemplar" => "HT",
        "DarkTemplar" => "DT",
        "Immortal" => "IMM",
        "Colossus" => "COL",
        "Disruptor" | "DisruptorPhased" => "DIS",
        "Archon" => "ARC",
        "Observer" | "ObserverSiegeMode" => "OBS",
        "WarpPrism" | "WarpPrismPhasing" => "PRI",
        "Phoenix" => "PHX",
        "VoidRay" => "VR",
        "Oracle" => "ORA",
        "Tempest" => "TMP",
        "Carrier" => "CAR",
        "Mothership" => "MS",
        // Zerg
        "Drone" => "DRO",
        "Queen" => "QUE",
        "Zergling" => "ZGL",
        "Baneling" | "BanelingCocoon" => "BLN",
        "Roach" | "RoachBurrowed" => "ROA",
        "Ravager" | "RavagerCocoon" => "RAV",
        "Hydralisk" | "HydraliskBurrowed" => "HYD",
        "LurkerMP" | "LurkerMPBurrowed" | "LurkerMPEgg" => "LUR",
        "Mutalisk" => "MUT",
        "Corruptor" => "COR",
        "BroodLord" | "BroodLordCocoon" => "BL",
        "Infestor" | "InfestorBurrowed" => "INF",
        "SwarmHostMP" | "SwarmHostBurrowedMP" => "SH",
        "Viper" => "VIP",
        "Ultralisk" | "UltraliskBurrowed" => "ULT",
        "Overlord" | "OverlordTransport" => "OVL",
        "Overseer" | "OverseerSiegeMode" => "OVS",
        // Neutrals / shared spawns (raramente aparecem no painel)
        "Larva" => "LAR",
        "Interceptor" => "INT",
        "AutoTurret" => "TUR",
        "Locust" | "LocustMP" | "LocustMPFlying" => "LOC",
        "Broodling" => "BRO",
        "Changeling"
        | "ChangelingMarine"
        | "ChangelingMarineShield"
        | "ChangelingZealot"
        | "ChangelingZergling"
        | "ChangelingZerglingWings" => "CHG",
        other => {
            return other
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(3)
                .collect::<String>()
                .to_uppercase();
        }
    }
    .to_string()
}

/// Colapsa variantes de estado (voando, abaixada) no tipo base. Um
/// `CommandCenterFlying` é o mesmo edifício físico que um
/// `CommandCenter` pousado — mostrar os dois como chips separados só
/// polui o painel durante transições de relocação. Retorna string vazia
/// pra estruturas desconhecidas — caller deve filtrar.
pub(super) fn structure_canonical(name: &str) -> &'static str {
    match name {
        "CommandCenter" | "CommandCenterFlying" => "CommandCenter",
        "OrbitalCommand" | "OrbitalCommandFlying" => "OrbitalCommand",
        "Barracks" | "BarracksFlying" => "Barracks",
        "Factory" | "FactoryFlying" => "Factory",
        "Starport" | "StarportFlying" => "Starport",
        "SupplyDepot" | "SupplyDepotLowered" => "SupplyDepot",
        "PlanetaryFortress" => "PlanetaryFortress",
        "Refinery" => "Refinery",
        "EngineeringBay" => "EngineeringBay",
        "Armory" => "Armory",
        "FusionCore" => "FusionCore",
        "GhostAcademy" => "GhostAcademy",
        "Bunker" => "Bunker",
        "MissileTurret" => "MissileTurret",
        "SensorTower" => "SensorTower",
        "BarracksTechLab" => "BarracksTechLab",
        "FactoryTechLab" => "FactoryTechLab",
        "StarportTechLab" => "StarportTechLab",
        "BarracksReactor" => "BarracksReactor",
        "FactoryReactor" => "FactoryReactor",
        "StarportReactor" => "StarportReactor",
        "Hatchery" => "Hatchery",
        "Lair" => "Lair",
        "Hive" => "Hive",
        "Extractor" => "Extractor",
        "SpawningPool" => "SpawningPool",
        "RoachWarren" => "RoachWarren",
        "HydraliskDen" => "HydraliskDen",
        "BanelingNest" => "BanelingNest",
        "EvolutionChamber" => "EvolutionChamber",
        "Spire" => "Spire",
        "GreaterSpire" => "GreaterSpire",
        "InfestationPit" => "InfestationPit",
        "UltraliskCavern" => "UltraliskCavern",
        "NydusNetwork" => "NydusNetwork",
        "NydusCanal" => "NydusCanal",
        "LurkerDen" => "LurkerDen",
        "SpineCrawler" => "SpineCrawler",
        "SporeCrawler" => "SporeCrawler",
        "Nexus" => "Nexus",
        "Pylon" => "Pylon",
        "Assimilator" => "Assimilator",
        "Gateway" => "Gateway",
        "WarpGate" => "WarpGate",
        "Forge" => "Forge",
        "CyberneticsCore" => "CyberneticsCore",
        "TwilightCouncil" => "TwilightCouncil",
        "Stargate" => "Stargate",
        "RoboticsFacility" => "RoboticsFacility",
        "TemplarArchive" => "TemplarArchive",
        "DarkShrine" => "DarkShrine",
        "RoboticsBay" => "RoboticsBay",
        "FleetBeacon" => "FleetBeacon",
        "PhotonCannon" => "PhotonCannon",
        "ShieldBattery" => "ShieldBattery",
        _ => "",
    }
}

/// Mapeia nome canônico de estrutura → ícone PNG embutido. Filenames
/// casam com o nome canônico exatamente (ex.: `Barracks.png`,
/// `CommandCenter.png`). Alguns pares morph+origem compartilham sprite:
/// Terran Tech Lab / Reactor reutilizam o mesmo módulo em
/// Barracks/Factory/Starport, e Gateway/WarpGate também compartilham —
/// WarpGate é morph do Gateway, a diferenciação fica a cargo do tooltip
/// localizado. Morphs Zerg na linha Hatch→Lair→Hive e Spire→GreaterSpire
/// têm sprites próprios. Retorna `None` apenas pras estruturas ainda
/// sem arte (todas cobertas agora — fallback é defensivo).
pub(super) fn structure_icon(canonical: &str) -> Option<egui::ImageSource<'static>> {
    use egui::include_image;
    let src = match canonical {
        // Terran — base
        "CommandCenter" => {
            include_image!("../../../../assets/buildings/terran/CommandCenter.png")
        }
        "OrbitalCommand" => {
            include_image!("../../../../assets/buildings/terran/OrbitalCommand.png")
        }
        "PlanetaryFortress" => {
            include_image!("../../../../assets/buildings/terran/PlanetaryFortress.png")
        }
        "SupplyDepot" => include_image!("../../../../assets/buildings/terran/SupplyDepot.png"),
        "Refinery" => include_image!("../../../../assets/buildings/terran/Refinery.png"),
        // Terran — produção
        "Barracks" => include_image!("../../../../assets/buildings/terran/Barracks.png"),
        "Factory" => include_image!("../../../../assets/buildings/terran/Factory.png"),
        "Starport" => include_image!("../../../../assets/buildings/terran/Starport.png"),
        // Terran — tecnologia
        "EngineeringBay" => {
            include_image!("../../../../assets/buildings/terran/EngineeringBay.png")
        }
        "Armory" => include_image!("../../../../assets/buildings/terran/Armory.png"),
        "FusionCore" => include_image!("../../../../assets/buildings/terran/FusionCore.png"),
        "GhostAcademy" => include_image!("../../../../assets/buildings/terran/GhostAcademy.png"),
        // Terran — defesa
        "Bunker" => include_image!("../../../../assets/buildings/terran/Bunker.png"),
        "MissileTurret" => {
            include_image!("../../../../assets/buildings/terran/MissileTurret.png")
        }
        "SensorTower" => include_image!("../../../../assets/buildings/terran/SensorTower.png"),
        // Terran — add-ons (sprite compartilhado por tipo)
        "BarracksTechLab" | "FactoryTechLab" | "StarportTechLab" => {
            include_image!("../../../../assets/buildings/terran/TechLab.png")
        }
        "BarracksReactor" | "FactoryReactor" | "StarportReactor" => {
            include_image!("../../../../assets/buildings/terran/Reactor.png")
        }
        // Protoss — base
        "Nexus" => include_image!("../../../../assets/buildings/protoss/Nexus.png"),
        "Pylon" => include_image!("../../../../assets/buildings/protoss/Pylon.png"),
        "Assimilator" => {
            include_image!("../../../../assets/buildings/protoss/Assimilator.png")
        }
        // Protoss — produção/tecnologia. Gateway/WarpGate compartilham
        // sprite — WarpGate é morph do Gateway; tooltip localizado
        // (`Gateway` vs `Warp Gate`) é que diferencia.
        "Gateway" | "WarpGate" => {
            include_image!("../../../../assets/buildings/protoss/Gateway.png")
        }
        "Forge" => include_image!("../../../../assets/buildings/protoss/Forge.png"),
        "CyberneticsCore" => {
            include_image!("../../../../assets/buildings/protoss/CyberneticsCore.png")
        }
        "TwilightCouncil" => {
            include_image!("../../../../assets/buildings/protoss/TwilightCouncil.png")
        }
        "Stargate" => include_image!("../../../../assets/buildings/protoss/Stargate.png"),
        "RoboticsFacility" => {
            include_image!("../../../../assets/buildings/protoss/RoboticsFacility.png")
        }
        "TemplarArchive" => {
            include_image!("../../../../assets/buildings/protoss/TemplarArchive.png")
        }
        "DarkShrine" => include_image!("../../../../assets/buildings/protoss/DarkShrine.png"),
        "RoboticsBay" => {
            include_image!("../../../../assets/buildings/protoss/RoboticsBay.png")
        }
        "FleetBeacon" => {
            include_image!("../../../../assets/buildings/protoss/FleetBeacon.png")
        }
        // Protoss — defesa
        "PhotonCannon" => {
            include_image!("../../../../assets/buildings/protoss/PhotonCannon.png")
        }
        "ShieldBattery" => {
            include_image!("../../../../assets/buildings/protoss/ShieldBattery.png")
        }
        // Zerg — base (morphs na linha Hatch→Lair→Hive e Spire→GreaterSpire
        // têm sprites distintos).
        "Hatchery" => include_image!("../../../../assets/buildings/zerg/Hatchery.png"),
        "Lair" => include_image!("../../../../assets/buildings/zerg/Lair.png"),
        "Hive" => include_image!("../../../../assets/buildings/zerg/Hive.png"),
        "Extractor" => include_image!("../../../../assets/buildings/zerg/Extractor.png"),
        // Zerg — produção/tecnologia
        "SpawningPool" => {
            include_image!("../../../../assets/buildings/zerg/SpawningPool.png")
        }
        "RoachWarren" => include_image!("../../../../assets/buildings/zerg/RoachWarren.png"),
        "HydraliskDen" => {
            include_image!("../../../../assets/buildings/zerg/HydraliskDen.png")
        }
        "BanelingNest" => {
            include_image!("../../../../assets/buildings/zerg/BanelingNest.png")
        }
        "EvolutionChamber" => {
            include_image!("../../../../assets/buildings/zerg/EvolutionChamber.png")
        }
        "Spire" => include_image!("../../../../assets/buildings/zerg/Spire.png"),
        "GreaterSpire" => {
            include_image!("../../../../assets/buildings/zerg/GreaterSpire.png")
        }
        "InfestationPit" => {
            include_image!("../../../../assets/buildings/zerg/InfestationPit.png")
        }
        "UltraliskCavern" => {
            include_image!("../../../../assets/buildings/zerg/UltraliskCavern.png")
        }
        "NydusNetwork" => {
            include_image!("../../../../assets/buildings/zerg/NydusNetwork.png")
        }
        "NydusCanal" => include_image!("../../../../assets/buildings/zerg/NydusCanal.png"),
        "LurkerDen" => include_image!("../../../../assets/buildings/zerg/LurkerDen.png"),
        // Zerg — defesa (sessile after root, mesma sprite)
        "SpineCrawler" => {
            include_image!("../../../../assets/buildings/zerg/SpineCrawler.png")
        }
        "SporeCrawler" => {
            include_image!("../../../../assets/buildings/zerg/SporeCrawler.png")
        }
        _ => return None,
    };
    Some(src)
}

/// Abreviação fixa de 3 letras pra estruturas (placeholder até sprites).
pub(super) fn structure_abbrev(canonical: &str) -> &'static str {
    match canonical {
        // Terran
        "CommandCenter" => "CC",
        "OrbitalCommand" => "ORB",
        "PlanetaryFortress" => "PF",
        "SupplyDepot" => "SD",
        "Refinery" => "REF",
        "Barracks" => "BAR",
        "Factory" => "FAC",
        "Starport" => "STP",
        "EngineeringBay" => "EB",
        "Armory" => "ARM",
        "FusionCore" => "FC",
        "GhostAcademy" => "GA",
        "Bunker" => "BNK",
        "MissileTurret" => "MTR",
        "SensorTower" => "SNT",
        "BarracksTechLab" => "BTL",
        "FactoryTechLab" => "FTL",
        "StarportTechLab" => "STL",
        "BarracksReactor" => "BRC",
        "FactoryReactor" => "FRC",
        "StarportReactor" => "SRC",
        // Zerg
        "Hatchery" => "HAT",
        "Lair" => "LAI",
        "Hive" => "HIV",
        "Extractor" => "EXT",
        "SpawningPool" => "SPL",
        "RoachWarren" => "RW",
        "HydraliskDen" => "HD",
        "BanelingNest" => "BN",
        "EvolutionChamber" => "EVO",
        "Spire" => "SPI",
        "GreaterSpire" => "GSP",
        "InfestationPit" => "IP",
        "UltraliskCavern" => "UC",
        "NydusNetwork" => "NN",
        "NydusCanal" => "NYD",
        "LurkerDen" => "LD",
        "SpineCrawler" => "SPC",
        "SporeCrawler" => "SPO",
        // Protoss
        "Nexus" => "NEX",
        "Pylon" => "PYL",
        "Assimilator" => "ASM",
        "Gateway" => "GW",
        "WarpGate" => "WG",
        "Forge" => "FRG",
        "CyberneticsCore" => "CYB",
        "TwilightCouncil" => "TC",
        "Stargate" => "SG",
        "RoboticsFacility" => "RF",
        "TemplarArchive" => "TA",
        "DarkShrine" => "DS",
        "RoboticsBay" => "RB",
        "FleetBeacon" => "FB",
        "PhotonCannon" => "PC",
        "ShieldBattery" => "SB",
        _ => "STR",
    }
}
