// Decodificador TGA mínimo, suficiente para os `Minimap.tga` que a
// Blizzard embute nos `.SC2Map`/`.s2ma`. Cobre os formatos que aparecem
// na prática:
//
//   - Image Type 2  : truecolor sem compressão (24/32 bpp BGR/BGRA)
//   - Image Type 10 : truecolor com RLE        (24/32 bpp BGR/BGRA)
//
// Devolve sempre RGBA8 com origem no canto superior-esquerdo (já invertendo
// linhas se o flag de origem do TGA estiver no canto inferior).

use super::MapImage;

const HEADER_LEN: usize = 18;

pub fn decode_tga(bytes: &[u8]) -> Result<MapImage, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!("TGA muito curto: {} bytes", bytes.len()));
    }

    let id_length = bytes[0] as usize;
    let color_map_type = bytes[1];
    let image_type = bytes[2];
    // bytes[3..8]   : color map specification (ignorado, color_map_type=0)
    // bytes[8..12]  : x/y origin (ignorado)
    let width = u16::from_le_bytes([bytes[12], bytes[13]]) as u32;
    let height = u16::from_le_bytes([bytes[14], bytes[15]]) as u32;
    let bpp = bytes[16];
    let descriptor = bytes[17];

    if color_map_type != 0 {
        return Err(format!("TGA com color map não suportado: {color_map_type}"));
    }
    if !matches!(image_type, 2 | 10) {
        return Err(format!("TGA image type não suportado: {image_type}"));
    }
    if !matches!(bpp, 24 | 32) {
        return Err(format!("TGA bpp não suportado: {bpp}"));
    }
    if width == 0 || height == 0 {
        return Err(format!("TGA com dimensão zero: {width}x{height}"));
    }

    let bytes_per_px = (bpp / 8) as usize;
    let pixel_count = (width as usize) * (height as usize);
    let payload_start = HEADER_LEN + id_length;
    if payload_start > bytes.len() {
        return Err("TGA payload offset fora dos bounds".to_string());
    }
    let payload = &bytes[payload_start..];

    let bgra = match image_type {
        2 => decode_uncompressed(payload, pixel_count, bytes_per_px)?,
        10 => decode_rle(payload, pixel_count, bytes_per_px)?,
        _ => unreachable!(),
    };

    // Converte BGR(A) → RGBA8.
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for px in bgra.chunks_exact(bytes_per_px) {
        let b = px[0];
        let g = px[1];
        let r = px[2];
        let a = if bytes_per_px == 4 { px[3] } else { 255 };
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(a);
    }

    // Bit 5 do descriptor: 0 = origem inferior-esquerda (precisa flipar),
    // 1 = origem superior-esquerda (já está como queremos).
    let top_origin = (descriptor & 0b0010_0000) != 0;
    if !top_origin {
        let row_bytes = (width as usize) * 4;
        let mut flipped = Vec::with_capacity(rgba.len());
        for row in (0..height as usize).rev() {
            let start = row * row_bytes;
            flipped.extend_from_slice(&rgba[start..start + row_bytes]);
        }
        rgba = flipped;
    }

    Ok(MapImage {
        width,
        height,
        rgba,
    })
}

fn decode_uncompressed(
    payload: &[u8],
    pixel_count: usize,
    bytes_per_px: usize,
) -> Result<Vec<u8>, String> {
    let needed = pixel_count * bytes_per_px;
    if payload.len() < needed {
        return Err(format!(
            "TGA payload truncado: precisa {needed}, tem {}",
            payload.len()
        ));
    }
    Ok(payload[..needed].to_vec())
}

fn decode_rle(
    payload: &[u8],
    pixel_count: usize,
    bytes_per_px: usize,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(pixel_count * bytes_per_px);
    let mut i = 0;
    while out.len() < pixel_count * bytes_per_px {
        if i >= payload.len() {
            return Err("TGA RLE: payload acabou antes do esperado".to_string());
        }
        let header = payload[i];
        i += 1;
        let count = ((header & 0x7F) as usize) + 1;
        if header & 0x80 != 0 {
            // Run-length packet: 1 pixel repetido `count` vezes.
            if i + bytes_per_px > payload.len() {
                return Err("TGA RLE: pacote RLE truncado".to_string());
            }
            let px = &payload[i..i + bytes_per_px];
            for _ in 0..count {
                out.extend_from_slice(px);
            }
            i += bytes_per_px;
        } else {
            // Raw packet: `count` pixels literais.
            let raw_len = count * bytes_per_px;
            if i + raw_len > payload.len() {
                return Err("TGA RLE: pacote raw truncado".to_string());
            }
            out.extend_from_slice(&payload[i..i + raw_len]);
            i += raw_len;
        }
    }
    out.truncate(pixel_count * bytes_per_px);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u16, height: u16, image_type: u8, bpp: u8, top_origin: bool) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN];
        h[2] = image_type;
        h[12..14].copy_from_slice(&width.to_le_bytes());
        h[14..16].copy_from_slice(&height.to_le_bytes());
        h[16] = bpp;
        h[17] = if top_origin { 0b0010_0000 } else { 0 };
        h
    }

    #[test]
    fn decodes_uncompressed_32bpp_top_origin() {
        // 2x1: pixel A (azul, alfa 128), pixel B (vermelho, alfa 255).
        // BGRA na fita: 255,0,0,128 | 0,0,255,255
        let mut bytes = header(2, 1, 2, 32, true);
        bytes.extend_from_slice(&[255, 0, 0, 128, 0, 0, 255, 255]);
        let img = decode_tga(&bytes).expect("decode");
        assert_eq!((img.width, img.height), (2, 1));
        // RGBA esperado: 0,0,255,128 | 255,0,0,255
        assert_eq!(img.rgba, vec![0, 0, 255, 128, 255, 0, 0, 255]);
    }

    #[test]
    fn decodes_uncompressed_24bpp_bottom_origin_flips_rows() {
        // 1x2 BGR, origem inferior-esquerda. Pixel inferior = vermelho,
        // pixel superior = verde. Após flip esperamos verde primeiro.
        let mut bytes = header(1, 2, 2, 24, false);
        // Linha inferior: vermelho (BGR 0,0,255)
        bytes.extend_from_slice(&[0, 0, 255]);
        // Linha superior: verde (BGR 0,255,0)
        bytes.extend_from_slice(&[0, 255, 0]);
        let img = decode_tga(&bytes).expect("decode");
        assert_eq!((img.width, img.height), (1, 2));
        // Após flip: linha 0 = verde, linha 1 = vermelho.
        assert_eq!(
            img.rgba,
            vec![0, 255, 0, 255, 255, 0, 0, 255],
        );
    }

    #[test]
    fn decodes_rle_run_and_raw_packets() {
        // 4x1, 32bpp, RLE, top origin.
        // Pacote 1: run de 2 pixels azul (BGRA 255,0,0,255)
        //   header = 0x80 | (2-1) = 0x81
        // Pacote 2: raw com 2 pixels (vermelho, verde)
        //   header = 0x00 | (2-1) = 0x01
        let mut bytes = header(4, 1, 10, 32, true);
        bytes.push(0x81);
        bytes.extend_from_slice(&[255, 0, 0, 255]);
        bytes.push(0x01);
        bytes.extend_from_slice(&[0, 0, 255, 255]); // vermelho
        bytes.extend_from_slice(&[0, 255, 0, 255]); // verde
        let img = decode_tga(&bytes).expect("decode");
        assert_eq!((img.width, img.height), (4, 1));
        assert_eq!(
            img.rgba,
            vec![
                0, 0, 255, 255,
                0, 0, 255, 255,
                255, 0, 0, 255,
                0, 255, 0, 255,
            ],
        );
    }

    #[test]
    fn rejects_unsupported_image_type() {
        let bytes = header(1, 1, 1, 32, true); // type 1 = colormap
        assert!(decode_tga(&bytes).is_err());
    }

    #[test]
    fn rejects_truncated_payload() {
        let mut bytes = header(2, 1, 2, 32, true);
        bytes.extend_from_slice(&[255, 0, 0, 128]); // só 1 pixel de 2
        assert!(decode_tga(&bytes).is_err());
    }
}
