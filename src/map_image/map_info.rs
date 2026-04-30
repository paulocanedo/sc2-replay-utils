// Parser do arquivo `MapInfo` dentro do MPQ de um `.SC2Map` / `.s2ma`.
//
// O `MapInfo` é o lugar canônico para a **área jogável** do mapa: o
// retângulo `(min_x, min_y, max_x, max_y)` em coordenadas de tile que
// o jogo realmente expõe ao jogador (a borda fora desse retângulo é
// "unplayable margin", usada para ambientação visual e para reservar
// espaço para a câmera não cair fora do mundo).
//
// Hoje a UI deriva esse retângulo heuristicamente a partir das
// posições observadas em `entity_events` (com margem de 4 tiles).
// Esse caminho continua ativo como fallback para mapas custom sem
// `.SC2Map` no Battle.net Cache; quando o `MapInfo` está disponível,
// preferimos seus valores exatos.
//
// **Layout binário** (validado em duas amostras de ladder, version=39):
//
// ```
// 0x00   "IpaM"          magic — "MapI" como u32 LE, escrito on-disk
//                        com bytes 0x49 0x70 0x61 0x4D
// 0x04   version         u32 LE (39 nas amostras)
// 0x08   build/hash      u32 LE (varia, ignorado)
// 0x0c   reserved        u32 LE (sempre 0 nas amostras)
// 0x10   width           u32 LE — tamanho da grade completa em tiles
// 0x14   height          u32 LE — idem
// 0x18..0x29   18 bytes  flags de preview/minimap (não usados aqui)
// 0x2a   tileset         cstring (zero-terminated, ex.: "Dark")
// ?      planet          cstring (ex.: "ShadowCorpsPlatform", "Slayn")
// ?      playable_min_x  u32 LE
// ?      playable_min_y  u32 LE
// ?      playable_max_x  u32 LE
// ?      playable_max_y  u32 LE
// ```
//
// O parser é tolerante: qualquer divergência de magic/dimensões fora
// de range/cstring sem terminador devolve `Err` e o caller cai no
// caminho heurístico.

#[derive(Clone, Debug)]
pub struct MapInfo {
    /// Tamanho da grade completa do mapa, em tiles. Inclui a margem
    /// unplayable além da `playable area`. Tipicamente entre 64 e 256.
    pub width: u32,
    pub height: u32,
    /// Retângulo da área jogável, em coordenadas de tile.
    /// `(min_x, min_y)` é o canto inferior-esquerdo (Y crescente
    /// "para cima" na convenção do jogo); `(max_x, max_y)` é
    /// exclusive (a célula `max_x` em si está fora). Para uso na UI
    /// tratamos como um retângulo `[min, max)` sobre tiles.
    pub playable_min_x: u32,
    pub playable_min_y: u32,
    pub playable_max_x: u32,
    pub playable_max_y: u32,
}

/// Magic em disco. A Blizzard escreve a string "MapI" como u32 LE
/// (mais comum em formatos derivados do MoPaQ/Storm), então a sequência
/// de bytes que vemos é a reversa: 0x49 0x70 0x61 0x4D = "IpaM".
const MAGIC: &[u8; 4] = b"IpaM";
const HEADER_SIZE: usize = 0x2a;

pub fn parse(bytes: &[u8]) -> Result<MapInfo, String> {
    if bytes.len() < HEADER_SIZE + 16 {
        return Err(format!(
            "MapInfo: too short ({} bytes)",
            bytes.len()
        ));
    }
    if &bytes[0..4] != MAGIC {
        return Err(format!(
            "MapInfo: bad magic {:?}",
            &bytes[0..4]
        ));
    }
    let width = read_u32(&bytes[0x10..0x14]);
    let height = read_u32(&bytes[0x14..0x18]);
    if !plausible_dim(width) || !plausible_dim(height) {
        return Err(format!(
            "MapInfo: implausible dimensions {width}x{height}"
        ));
    }

    let mut cursor = HEADER_SIZE;
    let _tileset = read_cstring(bytes, &mut cursor)?;
    let _planet = read_cstring(bytes, &mut cursor)?;
    if cursor + 16 > bytes.len() {
        return Err("MapInfo: truncated before playable area".into());
    }
    let playable_min_x = read_u32(&bytes[cursor..cursor + 4]);
    let playable_min_y = read_u32(&bytes[cursor + 4..cursor + 8]);
    let playable_max_x = read_u32(&bytes[cursor + 8..cursor + 12]);
    let playable_max_y = read_u32(&bytes[cursor + 12..cursor + 16]);

    if playable_min_x >= playable_max_x
        || playable_min_y >= playable_max_y
        || playable_max_x > width
        || playable_max_y > height
    {
        return Err(format!(
            "MapInfo: implausible playable rect ({playable_min_x},{playable_min_y})-({playable_max_x},{playable_max_y}) inside {width}x{height}"
        ));
    }

    Ok(MapInfo {
        width,
        height,
        playable_min_x,
        playable_min_y,
        playable_max_x,
        playable_max_y,
    })
}

fn read_u32(slice: &[u8]) -> u32 {
    u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]])
}

fn plausible_dim(d: u32) -> bool {
    (32..=512).contains(&d)
}

fn read_cstring(bytes: &[u8], cursor: &mut usize) -> Result<String, String> {
    let start = *cursor;
    while *cursor < bytes.len() && bytes[*cursor] != 0 {
        *cursor += 1;
    }
    if *cursor >= bytes.len() {
        return Err("MapInfo: unterminated cstring".into());
    }
    let s = std::str::from_utf8(&bytes[start..*cursor])
        .map_err(|e| format!("MapInfo: cstring not utf-8: {e}"))?
        .to_string();
    *cursor += 1;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture extraída de um `.s2ma` real (Ruby Rock LE):
    /// width=184, height=176, playable=(21,13)-(163,151).
    fn ruby_rock_fixture() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"IpaM");
        b.extend_from_slice(&27u32.to_le_bytes()); // version 0x27
        b.extend_from_slice(&80201u32.to_le_bytes()); // build/hash
        b.extend_from_slice(&0u32.to_le_bytes()); // reserved
        b.extend_from_slice(&184u32.to_le_bytes()); // width
        b.extend_from_slice(&176u32.to_le_bytes()); // height
        // 18 bytes of preview/minimap flags
        b.extend_from_slice(&[
            0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]);
        b.extend_from_slice(b"Dark\0");
        b.extend_from_slice(b"ShadowCorpsPlatform\0");
        b.extend_from_slice(&21u32.to_le_bytes());
        b.extend_from_slice(&13u32.to_le_bytes());
        b.extend_from_slice(&163u32.to_le_bytes());
        b.extend_from_slice(&151u32.to_le_bytes());
        b
    }

    #[test]
    fn parses_known_map() {
        let info = parse(&ruby_rock_fixture()).unwrap();
        assert_eq!(info.width, 184);
        assert_eq!(info.height, 176);
        assert_eq!(info.playable_min_x, 21);
        assert_eq!(info.playable_min_y, 13);
        assert_eq!(info.playable_max_x, 163);
        assert_eq!(info.playable_max_y, 151);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut b = ruby_rock_fixture();
        b[0] = b'X';
        assert!(parse(&b).is_err());
    }

    #[test]
    fn rejects_truncated() {
        let b = ruby_rock_fixture();
        assert!(parse(&b[..30]).is_err());
    }

    #[test]
    fn rejects_implausible_dims() {
        let mut b = ruby_rock_fixture();
        // zera width
        b[0x10..0x14].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse(&b).is_err());
    }

    #[test]
    fn rejects_playable_outside_grid() {
        let mut b = ruby_rock_fixture();
        // Ajusta playable_max_x para além de width (184).
        let pos = b.len() - 8; // playable_max_x está 8 bytes antes do final
        b[pos..pos + 4].copy_from_slice(&500u32.to_le_bytes());
        assert!(parse(&b).is_err());
    }
}
