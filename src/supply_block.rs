use crate::build_order::format_time;
use crate::replay::StatsSnapshot;

// ── Structs ───────────────────────────────────────────────────────────────────

pub struct SupplyBlockEntry {
    pub start_loop: u32,
    pub end_loop: u32,
    pub supply: i32, // supply_used no início do bloco
}

// ── Detecção ──────────────────────────────────────────────────────────────────

/// Detecta períodos de supply block nos snapshots de um jogador.
///
/// Um supply block ocorre quando `supply_used >= supply_made` e `supply_made < 200`.
/// `game_loops` é usado para fechar blocos ainda abertos ao fim da sequência.
pub fn extract_supply_blocks(
    snapshots: &[StatsSnapshot],
    game_loops: u32,
) -> Vec<SupplyBlockEntry> {
    let mut results = Vec::new();
    let mut in_block = false;
    let mut block_start_loop = 0u32;
    let mut block_supply = 0i32;

    for s in snapshots {
        // Pula snapshots iniciais sem dados reais
        if s.supply_used == 0 && s.supply_made == 0 {
            continue;
        }
        let blocked = s.supply_used >= s.supply_made && s.supply_made < 200;

        if !in_block && blocked {
            in_block = true;
            block_start_loop = s.game_loop;
            block_supply = s.supply_used;
        } else if in_block && !blocked {
            results.push(SupplyBlockEntry {
                start_loop: block_start_loop,
                end_loop: s.game_loop,
                supply: block_supply,
            });
            in_block = false;
        }
    }

    // Bloco ainda aberto no fim
    if in_block {
        results.push(SupplyBlockEntry {
            start_loop: block_start_loop,
            end_loop: game_loops,
            supply: block_supply,
        });
    }

    results
}

// ── Formatação CSV ────────────────────────────────────────────────────────────

/// Serializa os blocos como CSV de largura fixada.
/// Colunas: start, end, duration, supply.
pub fn to_supply_block_csv(entries: &[SupplyBlockEntry]) -> String {
    let rows: Vec<(String, String, String, String)> = entries
        .iter()
        .map(|e| {
            let duration = e.end_loop.saturating_sub(e.start_loop);
            (
                format_time(e.start_loop),
                format_time(e.end_loop),
                format_time(duration),
                e.supply.to_string(),
            )
        })
        .collect();

    let w_start = rows.iter().map(|(s, _, _, _)| s.len()).max().unwrap_or(0).max("start".len());
    let w_end = rows.iter().map(|(_, e, _, _)| e.len()).max().unwrap_or(0).max("end".len());
    let w_dur = rows.iter().map(|(_, _, d, _)| d.len()).max().unwrap_or(0).max("duration".len());
    let w_sup = rows.iter().map(|(_, _, _, s)| s.len()).max().unwrap_or(0).max("supply".len());

    let mut out = String::new();
    out.push_str(&format!(
        "{:<w_start$}, {:<w_end$}, {:<w_dur$}, {:<w_sup$}\n",
        "start", "end", "duration", "supply",
        w_start = w_start, w_end = w_end, w_dur = w_dur, w_sup = w_sup,
    ));
    for (start, end, dur, sup) in &rows {
        out.push_str(&format!(
            "{:<w_start$}, {:<w_end$}, {:<w_dur$}, {:<w_sup$}\n",
            start, end, dur, sup,
            w_start = w_start, w_end = w_end, w_dur = w_dur, w_sup = w_sup,
        ));
    }
    out
}
