// Paleta compartilhada da GUI.
//
// Centraliza a convenção visual do SC2 (player1 = vermelho, player2
// = azul) para que sidebar, build order, charts etc. usem exatamente
// as mesmas cores. Manter isso aqui evita que cada tab reinvente o
// esquema e garante consistência entre abas.

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

/// Fill e label para cards genéricos (ex: seção "Partida").
pub const CARD_FILL: Color32 = Color32::from_gray(30);
pub const LABEL_DIM: Color32 = Color32::from_gray(130);
