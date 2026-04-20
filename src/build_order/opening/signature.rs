//! Fallback de assinatura de supply. Usado quando nenhuma heurística
//! nomeada casa: em vez de inventar um rótulo errado, devolvemos os
//! primeiros marcos de supply observados (ex.: `"13 Pool, 15 Hatch"`).

use super::facts::WindowFacts;
use super::types::{Confidence, OpeningLabel};

/// Fallback textual em inglês para signature vazia (replay sem
/// nenhum evento antes da janela — muito raro, normalmente só ocorre
/// em replays de 30s).
const SIGNATURE_FALLBACK: &str = "Standard opening";

pub(super) fn fallback(f: &WindowFacts) -> OpeningLabel {
    if f.signature.is_empty() {
        return OpeningLabel {
            opening: SIGNATURE_FALLBACK.to_string(),
            follow_up: None,
            confidence: Confidence::Signature,
        };
    }

    // Até 3 marcos — mais que isso deixa de parecer "resumo".
    let parts: Vec<String> = f
        .signature
        .iter()
        .take(3)
        .map(|(supply, name)| format!("{} {}", supply, name))
        .collect();
    OpeningLabel {
        opening: parts.join(", "),
        follow_up: None,
        confidence: Confidence::Signature,
    }
}
