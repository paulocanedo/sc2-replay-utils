// Shared GUI palette.
//
// Centralises the SC2 visual convention (player1 = red, player2 = blue)
// so sidebar, build order, charts, etc. all pick from the same place
// instead of re-inventing the wheel. Also exposes neutral surface /
// border / accent tokens used by `tokens.rs` consumers.

use egui::Color32;

/// Cor do slot do jogador no SC2. O jogo sempre pinta o player1 de
/// vermelho e o player2 de azul na UI in-game; adotamos esse padrão
/// como identidade visual primária em toda a GUI.
pub fn player_slot_color(index: usize) -> Color32 {
    match index {
        0 => Color32::from_rgb(180, 75, 75),  // vermelho suave P1
        1 => Color32::from_rgb(75, 120, 185), // azul suave P2
        _ => Color32::from_gray(140),
    }
}

/// Versão mais clara da cor do slot — útil para plots (linhas em
/// fundo escuro ficam mais legíveis um pouco mais claras) e textos
/// coloridos em cards escuros.
pub fn player_slot_color_bright(index: usize) -> Color32 {
    match index {
        0 => Color32::from_rgb(240, 120, 120),
        1 => Color32::from_rgb(120, 170, 240),
        _ => Color32::from_gray(180),
    }
}

/// Fill sutil para o card do usuário — tint discreto da cor do slot
/// em vez de verde, para manter coesão com a borda.
pub fn user_fill(index: usize) -> Color32 {
    match index {
        0 => Color32::from_rgb(42, 30, 30), // warm tint
        1 => Color32::from_rgb(30, 34, 42), // cool tint
        _ => Color32::from_gray(38),
    }
}

/// Chip "Você" — neutro, sem competir com a cor do slot.
pub const USER_CHIP_BG: Color32 = Color32::from_rgb(55, 55, 55);
pub const USER_CHIP_FG: Color32 = Color32::from_rgb(200, 200, 200);

// ── Surfaces & borders ───────────────────────────────────────────────
//
// Three-tier surface scale lets the chrome read as "floating" above the
// content without over-committing to accents. `SURFACE` matches the
// central panel fill, `SURFACE_ALT` is used by the top/status bars,
// `SURFACE_RAISED` is the card fill that sits on top of either.

pub const SURFACE: Color32 = Color32::from_gray(22);
pub const SURFACE_ALT: Color32 = Color32::from_gray(26);
pub const SURFACE_RAISED: Color32 = Color32::from_gray(30);

/// Fill e label para cards genéricos (ex: seção "Partida").
/// Alias for `SURFACE_RAISED` — kept for existing call sites.
pub const CARD_FILL: Color32 = SURFACE_RAISED;

pub const BORDER: Color32 = Color32::from_gray(50);
pub const BORDER_STRONG: Color32 = Color32::from_gray(70);

// ── Labels ───────────────────────────────────────────────────────────

pub const LABEL_DIM: Color32 = Color32::from_gray(130);
pub const LABEL_DIMMER: Color32 = Color32::from_gray(100);
pub const LABEL_SOFT: Color32 = Color32::from_gray(170);
pub const LABEL_STRONG: Color32 = Color32::from_gray(200);

// ── Semantic accents ─────────────────────────────────────────────────

pub const ACCENT_SUCCESS: Color32 = Color32::from_rgb(140, 200, 110);
pub const ACCENT_WARNING: Color32 = Color32::from_rgb(230, 170, 60);
pub const ACCENT_DANGER: Color32 = Color32::from_rgb(220, 90, 90);

// ── Race colours ─────────────────────────────────────────────────────
//
// Distinct from the P1/P2 slot palette (red/blue) so "race" and
// "player" never collide visually. Consumed by the Library entry row
// border as well as any future race badges.

pub const RACE_TERRAN: Color32 = Color32::from_rgb(90, 130, 180); // steel blue
pub const RACE_PROTOSS: Color32 = Color32::from_rgb(120, 180, 100); // golden green
pub const RACE_ZERG: Color32 = Color32::from_rgb(160, 80, 150); // magenta purple

/// Returns the visual accent for the given race. Falls back to a
/// neutral grey for unknown races (observer, random-before-reveal,
/// mod replays, etc.).
pub fn race_color(race: &str) -> Color32 {
    match crate::utils::race_letter(race) {
        'T' => RACE_TERRAN,
        'P' => RACE_PROTOSS,
        'Z' => RACE_ZERG,
        _ => Color32::from_gray(100),
    }
}
