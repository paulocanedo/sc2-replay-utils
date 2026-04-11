// API de scrubbing — consultas O(log n) sobre as timelines já
// indexadas pelo parser.

use std::collections::HashMap;

use super::types::{PlayerTimeline, ReplayTimeline, StatsSnapshot, UpgradeEntry};

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
