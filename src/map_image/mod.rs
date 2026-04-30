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
mod map_info;
mod objects;

pub use locator::{resolve_from_cache_handles, resolve_map_file_default};
pub use map_info::MapInfo;
pub use objects::StartLocation;

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

/// Conjunto de dados extraídos do `.SC2Map`/`.s2ma`. `image` é o
/// produto principal (sempre presente em sucesso); `info` e
/// `start_locations` são "best effort": ausentes ou parse-fail
/// derrubam só esses campos, sem afetar a renderização do fundo.
pub struct MapAssets {
    pub image: MapImage,
    pub info: Option<MapInfo>,
    pub start_locations: Vec<StartLocation>,
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
pub fn load_for_replay(map_title: &str, cache_handles: &[String]) -> Result<MapAssets, String> {
    let mut last_err: Option<String> = None;
    for path in resolve_from_cache_handles(cache_handles) {
        match extract_assets(&path) {
            Ok(assets) => return Ok(assets),
            Err(e) => last_err = Some(e),
        }
    }
    if let Some(path) = resolve_map_file_default(map_title) {
        return extract_assets(&path);
    }
    Err(last_err.unwrap_or_else(|| format!("mapa não encontrado: {map_title:?}")))
}

/// Abre o MPQ do mapa **uma única vez** e extrai todos os assets que
/// alimentam a UI: imagem (`Minimap.tga`), `MapInfo` (área jogável) e
/// `Objects` (start locations).
///
/// O `Minimap.tga` é obrigatório — se falhar, devolve `Err` e o caller
/// cai no fundo cinza. `MapInfo` e `Objects` são best-effort: se a
/// extração ou o parse falhar, deixamos `info=None` / start_locations
/// vazio sem propagar erro (a timeline continua funcionando, só sem o
/// enriquecimento espacial).
///
/// `mpq_path` pode apontar para um `.SC2Map` ou `.s2ma` — ambos são
/// MPQs com a mesma estrutura interna.
pub fn extract_assets(mpq_path: &Path) -> Result<MapAssets, String> {
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

    let img = decode::decode_tga(&tga_bytes)?;
    let image = crop_to_content(img);

    let info = mpq
        .read_mpq_file_sector("MapInfo", true, &file_contents)
        .ok()
        .and_then(|(_, b)| match map_info::parse(&b) {
            Ok(info) => Some(info),
            Err(e) => {
                eprintln!("MapInfo parse: {e}");
                None
            }
        });

    let start_locations = mpq
        .read_mpq_file_sector("Objects", true, &file_contents)
        .ok()
        .map(|(_, b)| objects::parse(&b))
        .unwrap_or_default();

    Ok(MapAssets {
        image,
        info,
        start_locations,
    })
}

/// Recorta a imagem para o bounding box dos pixels não-pretos.
///
/// Os `Minimap.tga` da Blizzard são armazenados em texturas de tamanho
/// power-of-two (256×256, 1024×1024) e a área desenhada do mapa fica
/// **centrada com bordas pretas largas** — observado empiricamente:
/// Old Republic LE renderiza o conteúdo em `[58..198, 58..198]` dentro
/// de uma TGA de 256×256, ou seja só ~55% da textura tem terreno.
///
/// Sem o crop, a aba Timeline preencheria todo o canvas com a TGA
/// (incluindo as bordas pretas) e, como as posições das unidades vêm
/// de `playable_bounds` que mapeia o **conteúdo visível** do mapa,
/// elas apareceriam **dentro** das bordas pretas da textura, longe
/// do mapa propriamente dito. Recortando a TGA para o conteúdo, o
/// aspect ratio da textura passa a casar com o aspect dos
/// `playable_bounds` (medido em vários replays, a diferença é < 2%
/// para partidas que jogaram a área toda) e tudo se alinha.
///
/// Limiar: pixel é "preto" se R, G e B <= 8. Se a imagem inteira for
/// preta (não deveria acontecer com um Minimap.tga válido) ou o
/// bounding box for degenerado, devolve a imagem original.
fn crop_to_content(img: MapImage) -> MapImage {
    let (w, h) = (img.width, img.height);
    let row_bytes = (w * 4) as usize;
    let mut min_x = w;
    let mut max_x = 0u32;
    let mut min_y = h;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..h {
        let row = &img.rgba[(y as usize) * row_bytes..(y as usize + 1) * row_bytes];
        for x in 0..w {
            let i = (x as usize) * 4;
            if row[i] > 8 || row[i + 1] > 8 || row[i + 2] > 8 {
                if x < min_x {
                    min_x = x;
                }
                if x >= max_x {
                    max_x = x + 1;
                }
                if y < min_y {
                    min_y = y;
                }
                if y >= max_y {
                    max_y = y + 1;
                }
                any = true;
            }
        }
    }
    if !any || max_x <= min_x || max_y <= min_y {
        return img;
    }
    let cw = max_x - min_x;
    let ch = max_y - min_y;
    if cw < 4 || ch < 4 {
        return img;
    }
    let mut rgba = Vec::with_capacity((cw * ch * 4) as usize);
    for y in min_y..max_y {
        let row_start = (y as usize) * row_bytes + (min_x as usize) * 4;
        let row_end = row_start + (cw as usize) * 4;
        rgba.extend_from_slice(&img.rgba[row_start..row_end]);
    }
    MapImage {
        width: cw,
        height: ch,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_strips_uniform_black_borders() {
        // 6x6 com um quadrado vermelho 2x2 no centro (linhas 2..4, colunas 2..4).
        let w = 6u32;
        let h = 6u32;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 2..4 {
            for x in 2..4 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i] = 255;
                rgba[i + 1] = 0;
                rgba[i + 2] = 0;
                rgba[i + 3] = 255;
            }
        }
        let cropped = crop_to_content(MapImage {
            width: w,
            height: h,
            rgba,
        });
        // Crop é fallback no-op se < 4x4 (proteção contra resultado degenerado)
        // → aqui pulamos o crop, mas o teste seguinte cobre o caso real.
        assert_eq!((cropped.width, cropped.height), (6, 6));
    }

    #[test]
    fn crop_keeps_only_non_black_region() {
        // 10x10 com bloco branco em [3..7, 2..8].
        let w = 10u32;
        let h = 10u32;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 2..8 {
            for x in 3..7 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
                rgba[i + 3] = 255;
            }
        }
        let cropped = crop_to_content(MapImage {
            width: w,
            height: h,
            rgba,
        });
        assert_eq!((cropped.width, cropped.height), (4, 6));
        // Todos os pixels do crop devem ser brancos.
        for px in cropped.rgba.chunks_exact(4) {
            assert_eq!(px, &[255, 255, 255, 255]);
        }
    }

    #[test]
    fn crop_returns_original_when_all_black() {
        let w = 8u32;
        let h = 8u32;
        let rgba = vec![0u8; (w * h * 4) as usize];
        let cropped = crop_to_content(MapImage {
            width: w,
            height: h,
            rgba,
        });
        assert_eq!((cropped.width, cropped.height), (8, 8));
    }
}
