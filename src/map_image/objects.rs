// Parser do arquivo `Objects` dentro do MPQ de um `.SC2Map` / `.s2ma`.
//
// `Objects` é um XML com a lista de **objetos colocados pelo mapper**:
// doodads, decals, points, lights, etc. Para a timeline só nos interessam
// os `<ObjectPoint>` com `Type="StartLoc"` — são as posições de spawn de
// cada slot de jogador (em coordenadas de mundo, em tiles, com fração).
//
// Exemplo (Ruby Rock LE):
//
// ```xml
// <ObjectPoint Id="1130421289" Position="46.5,133.5,0" Scale="1,1,1"
//              Type="StartLoc" Name="Start Location 001" Color="0,0,0,0"/>
// <ObjectPoint Id="705445512" Position="137.5,30.5,0" Rotation="3.1413"
//              Scale="1,1,1" Type="StartLoc" Name="Start Location 002"
//              Color="0,0,0,0"/>
// ```
//
// O arquivo costuma ter ~1 MB (centenas de doodads). Como só queremos
// os pontos do tipo `StartLoc`, varremos linha a linha procurando o
// padrão `Type="StartLoc"` e extraímos `Position="x,y,z"` da mesma
// linha. Não introduzimos dependência de XML; o formato gerado pelo
// editor da Blizzard é estável (uma tag por linha) e o failure mode
// (não achar nada) é benigno: o caller cai no fluxo sem start markers.

#[derive(Clone, Copy, Debug)]
pub struct StartLocation {
    /// Coordenadas em tiles, com fração — o editor coloca os pontos
    /// no centro do tile (ex.: `46.5`).
    pub x: f32,
    pub y: f32,
}

pub fn parse(bytes: &[u8]) -> Vec<StartLocation> {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        // Conteúdo não-utf8 é improvável (a Blizzard sempre serializa
        // Objects com header `encoding="utf-8"`), mas se acontecer
        // tentamos lossy: a maioria das tags ainda virá inteira, e
        // os atributos de interesse (Type, Position) são ASCII puro.
        Err(_) => {
            return parse_str(&String::from_utf8_lossy(bytes));
        }
    };
    parse_str(s)
}

fn parse_str(s: &str) -> Vec<StartLocation> {
    let mut out = Vec::new();
    for line in s.lines() {
        if !line.contains("Type=\"StartLoc\"") {
            continue;
        }
        if let Some(pos) = extract_attr(line, "Position") {
            if let Some(point) = parse_position(pos) {
                out.push(point);
            }
        }
    }
    out
}

fn extract_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = line.find(&needle)? + needle.len();
    let end = line[start..].find('"')? + start;
    Some(&line[start..end])
}

fn parse_position(s: &str) -> Option<StartLocation> {
    // Formato "x,y,z" com floats.
    let mut parts = s.split(',');
    let x: f32 = parts.next()?.trim().parse().ok()?;
    let y: f32 = parts.next()?.trim().parse().ok()?;
    Some(StartLocation { x, y })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_locations_from_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<PlacedObjects Version="27">
    <ObjectDoodad Id="385588688" Type="Decal" Position="141,66,9.999"/>
    <ObjectPoint Id="1130421289" Position="46.5,133.5,0" Scale="1,1,1" Type="StartLoc" Name="Start Location 001" Color="0,0,0,0"/>
    <ObjectPoint Id="705445512" Position="137.5,30.5,0" Rotation="3.1413" Scale="1,1,1" Type="StartLoc" Name="Start Location 002" Color="0,0,0,0"/>
    <ObjectPoint Id="111" Position="80,80,0" Type="OtherKind"/>
</PlacedObjects>"#;
        let pts = parse(xml.as_bytes());
        assert_eq!(pts.len(), 2);
        assert!((pts[0].x - 46.5).abs() < 0.001);
        assert!((pts[0].y - 133.5).abs() < 0.001);
        assert!((pts[1].x - 137.5).abs() < 0.001);
        assert!((pts[1].y - 30.5).abs() < 0.001);
    }

    #[test]
    fn empty_when_no_start_locs() {
        let xml = r#"<PlacedObjects><ObjectDoodad Position="1,2,0"/></PlacedObjects>"#;
        assert!(parse(xml.as_bytes()).is_empty());
    }

    #[test]
    fn malformed_lines_skipped() {
        let xml = r#"<ObjectPoint Type="StartLoc"/>
<ObjectPoint Type="StartLoc" Position="abc,def,0"/>
<ObjectPoint Type="StartLoc" Position="10.5,20.5,0"/>"#;
        let pts = parse(xml.as_bytes());
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 10.5).abs() < 0.001);
    }
}
