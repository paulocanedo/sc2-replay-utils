// API pública para localizar e rasterizar a imagem de um mapa SC2.
//
// Mapas (.SC2Map e .s2ma) são arquivos MPQ que carregam um `Minimap.tga`
// pré-renderizado pela Blizzard. Este módulo:
//
//  1. Localiza um arquivo de mapa, em ordem de preferência:
//     a) Pelo `m_cacheHandles` do replay → caminho exato no Battle.net
//        Cache (cobre 100% dos mapas de ladder).
//     b) Pelo título do mapa → stem do filename em
//        `Documents\StarCraft II\Maps` (cobre mapas custom/instalados).
//  2. Abre o MPQ via `s2protocol::read_mpq` (mesma stack já usada para
//     replays).
//  3. Lê o sector `Minimap.tga` do archive e decodifica em RGBA8 (ver
//     `decode`).
//
// O retorno é um `MapImage` neutro de UI — a integração com `egui` fica
// em `gui/tabs/timeline.rs`, que faz upload pra GPU sob demanda.

mod decode;
mod locator;

pub use locator::{resolve_from_cache_handles, resolve_map_file_default};

use std::path::Path;

/// Imagem rasterizada de um mapa, em RGBA8 com origem no canto superior
/// esquerdo. Representação neutra de UI; consumers (egui, dump pra arquivo,
/// etc.) recebem isso e fazem o upload conforme precisam.
pub struct MapImage {
    pub width: u32,
    pub height: u32,
    /// Pixels em RGBA8, linha por linha. `len() == width * height * 4`.
    pub rgba: Vec<u8>,
}

/// Pipeline completo: localiza o arquivo do mapa do replay e extrai sua
/// imagem rasterizada. Conveniência usada por `LoadedReplay::load`.
///
/// Estratégia em ordem:
/// 1. Itera todos os `.s2ma` referenciados em `cache_handles` que
///    existem no Battle.net Cache. Tenta abrir/extrair cada um — o
///    primeiro que for um MPQ válido com `Minimap.tga` ganha. Os
///    stubs de mod (e.g. `Core.SC2Mod`, que ficam como texto
///    "Standard Data: ..." e não são MPQs) falham silenciosamente
///    e seguimos pro próximo handle.
/// 2. Fallback: lookup por título nas pastas de Maps instaladas.
pub fn load_for_replay(map_title: &str, cache_handles: &[String]) -> Result<MapImage, String> {
    let mut last_err: Option<String> = None;
    for path in resolve_from_cache_handles(cache_handles) {
        match extract_minimap(&path) {
            Ok(img) => return Ok(img),
            Err(e) => last_err = Some(e),
        }
    }
    if let Some(path) = resolve_map_file_default(map_title) {
        return extract_minimap(&path);
    }
    Err(last_err.unwrap_or_else(|| format!("mapa não encontrado: {map_title:?}")))
}

/// Extrai a imagem `Minimap.tga` de um arquivo MPQ de mapa SC2 e a
/// devolve decodificada em RGBA8.
///
/// `mpq_path` pode apontar para um `.SC2Map` ou `.s2ma` — ambos são
/// MPQs com a mesma estrutura interna.
pub fn extract_minimap(mpq_path: &Path) -> Result<MapImage, String> {
    let path_str = mpq_path
        .to_str()
        .ok_or_else(|| format!("caminho não é UTF-8: {}", mpq_path.display()))?;

    let (mpq, file_contents) =
        s2protocol::read_mpq(path_str).map_err(|e| format!("read_mpq: {e:?}"))?;

    // `force_decompress = true` para garantir que pegamos os bytes
    // descomprimidos do TGA, mesmo quando a Blizzard armazena o sector
    // sem compressão (a flag interna pode estar setada de qualquer jeito).
    let (_tail, tga_bytes) = mpq
        .read_mpq_file_sector("Minimap.tga", true, &file_contents)
        .map_err(|e| format!("read_mpq_file_sector(Minimap.tga): {e:?}"))?;

    if tga_bytes.is_empty() {
        return Err("Minimap.tga vazio ou ausente no MPQ".to_string());
    }

    decode::decode_tga(&tga_bytes)
}
