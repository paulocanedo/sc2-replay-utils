// API pública para localizar e rasterizar a imagem de um mapa SC2.
//
// Mapas (.SC2Map e .s2ma) são arquivos MPQ que carregam um `Minimap.tga`
// pré-renderizado pela Blizzard. Este módulo:
//
//  1. Localiza um arquivo de mapa por título (ver `locator`).
//  2. Abre o MPQ via `s2protocol::read_mpq` (mesma stack já usada para
//     replays).
//  3. Lê o sector `Minimap.tga` do archive e decodifica em RGBA8 (ver
//     `decode`).
//
// O retorno é um `MapImage` neutro de UI — a integração com `egui` fica
// em `gui/tabs/timeline.rs`, que faz upload pra GPU sob demanda.
//
// **Limitações conhecidas**: a resolução por título é por *stem* do
// filename (ver doc em `locator::resolve_map_file`). Mapas no Battle.net
// Cache têm nome em hash e não casam — só mapas instalados em
// `StarCraft II/Maps` resolvem por enquanto.

mod decode;
mod locator;

pub use locator::resolve_map_file_default;

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

/// Pipeline completo: localiza o arquivo do mapa pelo título e extrai
/// sua imagem rasterizada. Conveniência usada por `LoadedReplay::load`.
///
/// Usa o lookup cacheado (`resolve_map_file_default`) — a varredura
/// recursiva das pastas de Maps acontece **uma vez por processo**, no
/// primeiro replay carregado. Carregamentos seguintes pagam só o custo
/// de abrir o MPQ e decodificar o TGA.
pub fn load_for_title(map_title: &str) -> Result<MapImage, String> {
    let path = resolve_map_file_default(map_title)
        .ok_or_else(|| format!("mapa não encontrado: {map_title:?}"))?;
    extract_minimap(&path)
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
