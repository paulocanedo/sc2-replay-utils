//! Template engine para a funcionalidade "salvar como…" da biblioteca.
//!
//! Expande variáveis (`{datetime}`, `{map}`, `{p1}`, …) usando os
//! metadados parseados do replay. Único consumidor é `expand_save_name`
//! em `app/state.rs` — invocado quando o usuário copia replays marcados
//! para outra pasta e quer renomeá-los pelo template.

use crate::library::ParsedMeta;
use crate::utils::{race_letter, sanitize};

/// Template padrão preenchido no campo da toolbar de seleção múltipla.
pub const DEFAULT_TEMPLATE: &str = "{datetime}_{map}-{p1}({r1})_vs_{p2}({r2})_{loops}";

/// Expande o template usando os metadados de um replay.
/// Retorna `None` se o replay tiver menos de 2 jogadores.
pub fn expand_template(template: &str, meta: &ParsedMeta) -> Option<String> {
    if meta.players.len() < 2 {
        return None;
    }

    let datetime_compact = {
        let raw = meta.datetime.replace(['-', ':', 'T'], "");
        if raw.len() >= 12 { raw[..12].to_string() } else { raw }
    };

    let duration = format!(
        "{:02}m{:02}s",
        meta.duration_seconds / 60,
        meta.duration_seconds % 60,
    );

    let result = template
        .replace("{datetime}", &datetime_compact)
        .replace("{map}", &sanitize(&meta.map))
        .replace("{p1}", &sanitize(&meta.players[0].name))
        .replace("{p2}", &sanitize(&meta.players[1].name))
        .replace("{r1}", &race_letter(&meta.players[0].race).to_string())
        .replace("{r2}", &race_letter(&meta.players[1].race).to_string())
        .replace("{loops}", &meta.game_loops.to_string())
        .replace("{duration}", &duration);

    Some(format!("{result}.SC2Replay"))
}
