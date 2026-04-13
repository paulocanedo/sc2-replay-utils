# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Bug Fixes

- *(build_order)* uniformiza legenda com espaçamento e estilo consistentes
- *(build_order)* usa Cmd events reais para start time, corrigindo Chrono Boost
- *(gui/timeline)* alinha unidades ao Minimap.tga via playable bounds
- *(build-order)* corrige bugs do build order e adiciona teste golden CSV
- *(army-value)* corrigir legenda e eventos irrelevantes no gráfico
- *(supply-block)* exibir supply abaixo da barra quando espaço é curto
- *(army-value)* corrigir legibilidade das cores no fundo branco
- *(dump)* renomear x/y para pos_x/pos_y e adicionar --no-location
- *(dump)* gravar YAMLs no diretório de execução por padrão
- *(dump)* usar dirs::document_dir() para resolver pasta de Documentos

### Features

- *(rename)* adiciona tela de renomear replays em lote com template
- *(timeline)* layout lateral de stats, heatmap de câmera e toggle
- *(timeline)* adiciona viewport da câmera dos jogadores no minimapa
- *(build_order)* adiciona identificação de Inject Larva com hatchery alvo
- add locale system, SALT encoding, and UI refinements
- *(build_order)* melhora UI com filtros expandidos, busca inline, copiar por jogador e header estilo card
- *(gui)* redesign sidebar com resumo, player cards com borda lateral e seção detalhes
- *(library)* cache persistente de metadados em disco para startup rápido
- *(gui)* adiciona tela de créditos do autor no menu Ajuda > Sobre
- *(charts)* toggle incluir workers, supply blocks visuais e supply no tooltip
- *(charts)* melhora gráfico de army value com eixos formatados, tooltip e zoom
- *(gui)* distingue cancelled/destroyed no build order + ajustes timeline
- *(replay)* coleta amostras de movimento via UnitPositionsEvent
- *(map)* resolve mapas via cache handles e ajusta layout do mini-mapa
- *(balance)* tempos de build via BalanceData oficial do s2protocol
- *(map)* extrai Minimap.tga do .SC2Map e usa como fundo da Timeline
- *(gui/timeline)* mini-mapa por instante e nova aba Chat
- *(gui)* separar biblioteca e análise em telas distintas
- *(gui/build-order)* categorias, filtros, busca, legenda e tempo de início
- *(gui)* esquema de cores P1/P2 do SC2 consistente entre abas
- adicionar comando chat, renomear dump→all e corrigir ícones Terran
- *(mmr)* exibir MMR dos jogadores nos títulos dos gráficos
- *(build-order)* escala de supply com régua uniforme por unidade
- *(build-order)* empilhamento de ícones, estruturas acima do eixo e escala de supply
- *(build-order)* exibir upgrades acima do eixo X e adicionar ícones de construções
- *(icons)* adicionar ícones de upgrades Terran e remover unidades removidas
- template descritivo para nomes de arquivo de saída
- *(army-value)* antialiasing nas curvas de valor de exército
- *(army-value)* separar faixas de upgrade por jogador
- *(layout)* mover ícones para abaixo da linha do eixo X
- *(icons)* integrar ícones nas imagens de build order e army value
- fundo branco nas imagens e max-time em segundos
- *(supply-block)* exibir valor de supply no rótulo de cada bloco
- adicionar comando army-value com gráfico de valor de exército
- adicionar geração de PNG e comando supply-block
- *(build-order)* novo comando para extrair Build Order em CSV
- *(dump)* adicionar --latest para descobrir o replay mais recente
- suporte a variáveis de ambiente via .env
- *(dump)* adicionar --max-time para limitar coleta de eventos
- *(dump)* incluir tracker events no YAML por jogador
- *(dump)* aceitar arquivo único além de diretório

### Miscellaneous

- remover suporte CLI, manter apenas o binário GUI
- snapshot da GUI, production gap e ajustes pendentes
- ignorar arquivos YAML gerados pelo app
- adicionar .env.example com todas as variáveis documentadas

### Refactoring

- *(replay)* dividir replay.rs em submódulos por responsabilidade
- parser single-pass com ReplayTimeline unificada
- *(dump)* separar SC2_REPLAY_DIR de variáveis da ferramenta

