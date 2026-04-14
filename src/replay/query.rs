// API de scrubbing — consultas O(log n) sobre as timelines já
// indexadas pelo parser.

use std::collections::HashMap;

use super::types::{CameraPosition, PlayerTimeline, ReplayTimeline, StatsSnapshot, UpgradeEntry};

impl ReplayTimeline {
    /// Converte segundos para game loops usando o `loops_per_second`
    /// do replay.
    #[allow(dead_code)]
    pub fn loop_at_seconds(&self, secs: f64) -> u32 {
        (secs * self.loops_per_second).max(0.0).round() as u32
    }
}

impl PlayerTimeline {
    /// Último `StatsSnapshot` cujo game_loop é ≤ `game_loop`.
    /// Binary search → O(log n).
    pub fn stats_at(&self, game_loop: u32) -> Option<&StatsSnapshot> {
        let i = self.stats.partition_point(|s| s.game_loop <= game_loop);
        if i == 0 {
            None
        } else {
            Some(&self.stats[i - 1])
        }
    }

    /// Slice de upgrades pesquisados até `game_loop` (inclusivo).
    #[allow(dead_code)]
    pub fn upgrades_until(&self, game_loop: u32) -> &[UpgradeEntry] {
        let i = self.upgrades.partition_point(|u| u.game_loop <= game_loop);
        &self.upgrades[..i]
    }

    /// Nível de attack acumulado até `game_loop` (inclusivo).
    #[allow(dead_code)]
    pub fn attack_level_at(&self, game_loop: u32) -> u8 {
        let i = self
            .upgrade_cumulative
            .partition_point(|(l, _, _)| *l <= game_loop);
        if i == 0 {
            0
        } else {
            self.upgrade_cumulative[i - 1].1
        }
    }

    /// Nível de armor acumulado até `game_loop` (inclusivo).
    #[allow(dead_code)]
    pub fn armor_level_at(&self, game_loop: u32) -> u8 {
        let i = self
            .upgrade_cumulative
            .partition_point(|(l, _, _)| *l <= game_loop);
        if i == 0 {
            0
        } else {
            self.upgrade_cumulative[i - 1].2
        }
    }

    /// Quantas entidades de `entity_type` estão vivas em `game_loop`.
    #[allow(dead_code)]
    pub fn alive_count_at(&self, entity_type: &str, game_loop: u32) -> i32 {
        let Some(v) = self.alive_count.get(entity_type) else {
            return 0;
        };
        let i = v.partition_point(|(l, _)| *l <= game_loop);
        if i == 0 {
            0
        } else {
            v[i - 1].1
        }
    }

    /// Última posição conhecida da unidade `tag` em ou antes de
    /// `game_loop`. `None` se a unidade nunca foi amostrada nesse
    /// intervalo (ex.: estrutura ou tag fora do replay).
    #[allow(dead_code)]
    pub fn unit_position_at(&self, tag: i64, game_loop: u32) -> Option<(u8, u8)> {
        // `unit_positions` é ordenado por `game_loop`, mas conter
        // várias tags entrelaçadas. Walk linear filtrando por tag
        // — barato porque a Timeline cacheia o snapshot via
        // `last_known_positions` quando precisa de todas as tags.
        let mut last: Option<(u8, u8)> = None;
        for s in &self.unit_positions {
            if s.game_loop > game_loop {
                break;
            }
            if s.tag == tag {
                last = Some((s.x, s.y));
            }
        }
        last
    }

    /// Snapshot `tag → (x, y)` com a última posição conhecida de
    /// cada unidade em ou antes de `until_loop`. Pensado para
    /// consumers que precisam reconstruir todas as unidades vivas
    /// num instante (custo O(n) sobre `unit_positions`).
    pub fn last_known_positions(&self, until_loop: u32) -> HashMap<i64, (u8, u8)> {
        let mut out: HashMap<i64, (u8, u8)> = HashMap::new();
        for s in &self.unit_positions {
            if s.game_loop > until_loop {
                break;
            }
            out.insert(s.tag, (s.x, s.y));
        }
        out
    }

    /// Snapshot `tag → (x, y)` com posição interpolada linearmente
    /// entre as duas amostras adjacentes em `unit_positions` que
    /// envelopam `at_loop`. Quando o jogador está fora do intervalo
    /// amostrado de uma unidade (antes da primeira amostra ou depois
    /// da última), devolve a amostra mais próxima.
    ///
    /// Razão de existir: o SC2 emite `UnitPositionsEvent` muito
    /// esparsamente (tipicamente ~2-3 amostras por unidade ao longo
    /// da vida dela). Sem interpolação, as unidades aparentam
    /// "teleportar" no minimapa em vez de se mover. A interpolação
    /// linear assume movimento em linha reta entre amostras — boa
    /// aproximação visual mesmo quando a unidade fez detours, porque
    /// o intervalo entre amostras costuma ser curto comparado à
    /// escala do mapa.
    ///
    /// Custo: O(n) único sweep sobre `unit_positions`. Tags sem
    /// nenhuma amostra em `unit_positions` (ex.: estruturas) não
    /// aparecem no resultado — o consumer cai no fallback de posição
    /// de nascimento.
    pub fn interpolated_positions(&self, at_loop: u32) -> HashMap<i64, (f32, f32)> {
        // Por tag, mantemos a última amostra ≤ at_loop vista até agora.
        // Quando encontramos a primeira amostra > at_loop pra esse tag,
        // interpolamos e marcamos o tag como "fechado" (não atualizamos
        // mais). Isso evita um segundo passe ou um agrupamento prévio.
        let mut prev: HashMap<i64, (u32, u8, u8)> = HashMap::new();
        let mut out: HashMap<i64, (f32, f32)> = HashMap::new();
        for s in &self.unit_positions {
            if out.contains_key(&s.tag) {
                continue;
            }
            if s.game_loop <= at_loop {
                prev.insert(s.tag, (s.game_loop, s.x, s.y));
            } else if let Some(&(pl, px, py)) = prev.get(&s.tag) {
                let total = (s.game_loop - pl) as f32;
                let frac = if total > 0.0 {
                    (at_loop - pl) as f32 / total
                } else {
                    0.0
                };
                let x = px as f32 + (s.x as f32 - px as f32) * frac;
                let y = py as f32 + (s.y as f32 - py as f32) * frac;
                out.insert(s.tag, (x, y));
            }
            // Amostra > at_loop sem prev: a unidade ainda não foi
            // vista. Não emite nada — caller cai no fallback.
        }
        // Tags com prev mas sem amostra posterior: fica na última
        // posição conhecida.
        for (tag, (_, x, y)) in prev {
            out.entry(tag).or_insert((x as f32, y as f32));
        }
        out
    }

    /// Última posição conhecida da câmera em ou antes de `game_loop`.
    /// Binary search → O(log n). `None` antes do primeiro evento de
    /// câmera (tipicamente nos primeiros loops do replay).
    pub fn camera_at(&self, game_loop: u32) -> Option<&CameraPosition> {
        let i = self.camera_positions.partition_point(|c| c.game_loop <= game_loop);
        if i == 0 {
            None
        } else {
            Some(&self.camera_positions[i - 1])
        }
    }

    /// Capacidade de produção de workers em `game_loop`.
    /// As entradas em `worker_capacity` são deltas (+1/-1); aqui acumulamos
    /// até o ponto pedido. O custo é O(n) sobre uma lista pequena (poucas
    /// dezenas de eventos por jogador), aceitável dado que esta API serve
    /// scrubbing pontual da GUI, não loops quentes.
    #[allow(dead_code)]
    pub fn worker_capacity_at(&self, game_loop: u32) -> i32 {
        let i = self
            .worker_capacity
            .partition_point(|(l, _)| *l <= game_loop);
        self.worker_capacity[..i]
            .iter()
            .map(|(_, d)| *d)
            .sum::<i32>()
            .max(0)
    }
}
