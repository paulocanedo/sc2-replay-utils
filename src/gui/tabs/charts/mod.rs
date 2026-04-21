// Aba Gráficos — plot genérico de army (valor/quantidade, por jogador
// ou agrupado por tipo de unidade).
//
// O plot principal tem três eixos de configuração:
// - Métrica: Valor (minerals+gas, ou supply contribution por tipo) ou
//   Quantidade (contagem de entidades vivas).
// - Grupo: agregado por jogador (uma linha por jogador) ou agrupado por
//   tipo de unidade (uma linha por tipo; requer selecionar um jogador).
// - Amostragem: grade fixa de 5s, independente da resolução dos
//   eventos — evita serrilhado nas linhas ao usar dados do tracker.
//
// A identidade visual dos jogadores (P1 vermelho, P2 azul) permanece
// no modo agregado. No modo agrupado-por-tipo, cada linha ganha uma cor
// derivada do hash do tipo (estável entre renders).
//
// Organização:
//   - `classify`    — classificação de tipos, nomes canônicos, paleta de cores.
//   - `army`        — plot principal de army value/count.

mod army;
mod classify;

use egui::Ui;

use crate::config::AppConfig;
use crate::replay_state::LoadedReplay;

/// Métrica mostrada no eixo Y do plot principal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChartMetric {
    /// Valor em minerals+gas (agregado) ou supply contribution (por tipo).
    Value,
    /// Número de entidades vivas.
    Count,
}

/// Opções de exibição do plot principal. Mantidas em `AppState` para
/// persistir entre trocas de aba.
pub struct ArmyChartOptions {
    pub metric: ChartMetric,
    /// Incluir unidades de army no agregado (sem efeito no modo por tipo).
    pub show_army: bool,
    /// Incluir workers no agregado (sem efeito no modo por tipo —
    /// workers aparecem como seu próprio tipo).
    pub show_workers: bool,
    /// Uma linha por tipo de unidade (vs. uma linha por jogador).
    pub group_by_type: bool,
    /// Jogador selecionado para o modo por-tipo. Ignorado em agregado.
    pub grouped_player: usize,
    /// Mostrar o subgrid (marcas intermediárias entre os ticks rotulados).
    pub show_minor_grid: bool,
}

impl Default for ArmyChartOptions {
    fn default() -> Self {
        Self {
            metric: ChartMetric::Value,
            show_army: true,
            show_workers: false,
            group_by_type: false,
            grouped_player: 0,
            show_minor_grid: true,
        }
    }
}

pub fn show(
    ui: &mut Ui,
    loaded: &LoadedReplay,
    config: &AppConfig,
    army_opts: &mut ArmyChartOptions,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            army::army_value_plot(ui, loaded, config, army_opts);
        });
}
