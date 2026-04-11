use crate::replay::PlayerTimeline;

// ── Structs ───────────────────────────────────────────────────────────────────

pub struct SupplyBlockEntry {
    pub start_loop: u32,
    pub end_loop: u32,
    pub supply: i32, // supply_used no início do bloco
}

// ── Detecção ──────────────────────────────────────────────────────────────────

/// Detecta períodos de supply block nos stats de um jogador.
///
/// Um supply block ocorre quando `supply_used >= supply_made` e `supply_made < 200`.
/// `game_loops` é usado para fechar blocos ainda abertos ao fim da sequência.
pub fn extract_supply_blocks(
    player: &PlayerTimeline,
    game_loops: u32,
) -> Vec<SupplyBlockEntry> {
    let snapshots = player.stats.as_slice();
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

