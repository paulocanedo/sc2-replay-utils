# Parser — oportunidades de performance

Relatório da auditoria do parser em `src/replay/`. Gerado após a
migração de índices derivados para `finalize.rs` (canonicidade). Os
itens abaixo são otimizações **não aplicadas** nesta sessão — ficam
aqui para avaliação e implementação futura, priorizáveis pelo impacto
em replays longos (100k+ eventos) e pela frequência de uso na GUI.

Ordem sugerida: (2) e (5) reduzem alocações no hot path do parser;
(3) é refatoração maior que colhe o ganho dos anteriores.

## 1. `worker_capacity_at()` faz O(n) sum por chamada — ✓ implementado

`finalize.rs::derive_capacity_indices` agora popula
`worker_capacity_cumulative` e `army_capacity_cumulative` via
`build_cumulative` (prefix-sum sobre os deltas já ordenados), e
`worker_capacity_at` faz binary search sobre o cumulativo — O(log n).
Regressão protegida por `capacity_cumulative_matches_delta_sum` em
`src/replay/tests.rs`.

## 2. Clones de `String` em hot path do tracker

**Local**: [src/replay/tracker.rs](../src/replay/tracker.rs) — múltiplos pontos:

- `e.unit_type_name.clone()` em UnitBorn spawn instantâneo (linhas
  ~199, 222, 238, 268) — 2-3× por unidade nascida.
- `e.unit_type_name.clone()` em UnitBorn após UnitInit (linhas ~294, 302).
- `e.unit_type_name.clone()` em UnitInit (linhas ~360, 383).
- `state.entity_type.clone()` em UnitDone, UnitDied (linhas ~415, 475).
- `e.unit_type_name.clone()` em UnitTypeChange (linhas ~496, 523).
- `index_owner.insert` / `index_owner.get_mut(...).unit_type = e.unit_type_name.clone()`
  (linhas ~192, 302, 365, 523).

SC2 tem ~200 nomes únicos de unidade/estrutura. Cada replay tem
dezenas de milhares de eventos → dezenas de milhares de alocações
redundantes.

**Fix**: pool de `Arc<str>` local ao `process_tracker_events`. A
primeira vez que um nome aparece, aloca; próximas ocorrências
compartilham o mesmo `Arc`. `EntityEvent.entity_type` muda de
`String` para `Arc<str>` (item 5 abaixo).

**Impacto estimado**: redução de ~30-50% no tempo de parse de replays
longos (medir com `cargo bench` ou instrumentação ad-hoc).

## 3. Múltiplas passadas sobre `entity_events` em `finalize.rs`

**Local**: [src/replay/finalize.rs](../src/replay/finalize.rs) — `finalize_indices`:

Após a migração, cada `PlayerTimeline` sofre:

1. Passada para `alive_count` (linhas ~89-108).
2. Passada para `derive_capacity_indices` (`morph_started` HashSet + `started_abilities` HashMap + main loop) — **3 sub-passadas lineares**.
3. Passada para `build_creep_index` (`starts_at_loop` HashSet + main loop) — **2 sub-passadas**.

Total: ~6 passadas lineares sobre `entity_events` por jogador.

**Fix**: reescrever como 1 passada única com todas as regras acopladas.
O ideal teórico (documentado no plano de canonicidade) é:

```rust
for (i, ev) in events.iter().enumerate() {
    // alive_count
    // creep_index
    // worker_capacity / army_capacity (com detecção de morph inline)
    // worker_births
}
```

Os HashSets/HashMaps auxiliares (`morph_started`, `started_abilities`,
`starts_at_loop`) podem ser preenchidos numa passada de pré-processamento
muito barata (só `ProductionStarted`, que são uma fração dos eventos)
ou eliminados via olhadas para frente/atrás no índice.

**Impacto estimado**: 2-4× mais rápido em `finalize`. Finalize já é
baratíssimo comparado ao parse, então ganho absoluto é modesto (~5-10%
do tempo total de parse).

## 4. `upgrade_cumulative` com entradas duplicadas

**Local**: [src/replay/finalize.rs](../src/replay/finalize.rs) — `derive_upgrade_cumulative`

Empurra uma entry por `UpgradeEntry`, mesmo quando `(attack, armor)`
não muda (ex: Stimpack, Warp Gate, MercCompound — upgrades que não são
attack/armor). Em replays com 100+ upgrades, ~80% das entries podem
ter o mesmo `(attack, armor)` da anterior.

**Fix**:

```rust
fn derive_upgrade_cumulative(player: &mut PlayerTimeline) {
    let mut cur_attack: u8 = 0;
    let mut cur_armor: u8 = 0;
    for up in &player.upgrades {
        let level = upgrade_level(&up.name);
        let mut changed = false;
        if is_attack_upgrade(&up.name) && level > cur_attack {
            cur_attack = level;
            changed = true;
        }
        if is_armor_upgrade(&up.name) && level > cur_armor {
            cur_armor = level;
            changed = true;
        }
        if changed || player.upgrade_cumulative.is_empty() {
            player.upgrade_cumulative.push((up.game_loop, cur_attack, cur_armor));
        }
    }
}
```

**Cuidado**: o teste `upgrade_cumulative_monotonic` assume
`upgrade_cumulative.len() == upgrades.len()` (blindando o comportamento
atual). Ao aplicar o fix, relaxar para `len() <= upgrades.len()`.

**Impacto estimado**: economia de ~50-80% no tamanho de
`upgrade_cumulative`; queries `attack_level_at`/`armor_level_at` ficam
marginalmente mais rápidas (menos entries → `partition_point` mais
raso).

## 5. `EntityEvent.entity_type: String` — heap por evento

**Local**: [src/replay/types.rs:132](../src/replay/types.rs)

```rust
pub struct EntityEvent {
    ...
    pub entity_type: String,  // heap: 24 bytes + conteúdo
    ...
}
```

Em replays com 100k eventos, isso é ~2.4MB só nos headers de String +
o heap das strings em si. Combinado com item 2 (interning), vira
`Arc<str>`: 8 bytes por referência + 1 alocação compartilhada por nome
único.

**Fix**: `entity_type: Arc<str>`. Propaga mudança para:
- `creator_ability: Option<Arc<str>>` (mesmo benefício).
- `production_cmds[].ability: Arc<str>`.
- `inject_cmds[].target_type: Arc<str>`.
- `UpgradeEntry.name: Arc<str>`.
- `chat[].player_name` / `chat[].recipient` — menor volume, opcional.

**Impacto estimado**: redução de ~8MB em replays longos; tempo de
parse também cai por eliminar alocações.

## 6. `selection.control_groups[...].clone()` em `game.rs`

**Local**: [src/replay/game.rs:120,125,140](../src/replay/game.rs)

`ControlGroupUpdate::ESet` / `EAppend` / `ERecall` clonam o vetor
inteiro do control group ativo:

```rust
selection.control_groups[idx] = selection.control_groups[ACTIVE_UNITS_GROUP_IDX].clone();
```

Replays com APM alto (GM) têm dezenas de ControlGroupUpdate por
segundo. Cada clone aloca um `Vec<u32>`.

**Fix**: `mem::take` ou split-borrow quando a fonte não for lida
depois:

```rust
// Para ESet (sobrescreve sem ler de volta):
let src = selection.control_groups[ACTIVE_UNITS_GROUP_IDX].clone();
selection.control_groups[idx] = src;  // Ainda precisa clonar se src deve sobreviver.
```

O caso de `ESet`/`ESetAndSteal`/`ERecall` tipicamente *não* precisa
preservar a origem (ou pode ser reconstruída), então é candidato para
`std::mem::take` + reemitir se necessário. Análise caso a caso.

**Impacto estimado**: modesto (~5% do tempo de `process_game_events`).

## 7. `index_owner` guarda `String unit_type` clonado

**Local**: [src/replay/tracker.rs](../src/replay/tracker.rs) — `IndexEntry.unit_type: String`

Consumido por `game.rs` para resolver o tipo do produtor de um Cmd. A
cada UnitBorn/UnitInit/UnitTypeChange, clona `e.unit_type_name`. Mesma
dor do item 2 — resolvida pelo mesmo fix (pool de `Arc<str>`).

## 8. `creep_index.sort_by_key` removido (baseline)

Removido nesta sessão (era sort defensivo em `finalize.rs:177`). Valia
registro porque o comentário admitia explicitamente "defensivo" e
documentava que a ordenação vinha por construção. Boa prática: quando
um sort é "defensivo", vale investigar se pode ser eliminado por
invariante.

---

## Testes que faltam adicionar

Durante a auditoria foram identificados testes úteis que não fazem
parte da cobertura atual. Não bloqueiam nada hoje, mas protegem contra
regressões futuras:

- **Determinismo**: `parse_replay(path)` duas vezes → `ReplayTimeline`
  idêntico. Protege contra iteração não-determinística de HashMap em
  `finalize` afetando ordem de inserção em `alive_count`.
- **Conservação de tags**: cada `tag` aparece em no máximo um
  `ProductionStarted` por jogador. Morphs contam como 1 único tag ao
  longo da vida.
- **Conservação de entity_events**: `#ProductionFinished == #Died +
  #alive_final` para cada tipo. Já há partes disso em
  `alive_count_monotonic_for_morphs`; generalizar para todos os tipos.
- **`worker_capacity_at()` ≤ alive producers**: invariante que já está
  em `worker_capacity_matches_alive_producers`, mas via API
  `worker_capacity_at` (cobrindo o `.max(0)` final).
