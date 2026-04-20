//! Tipos públicos do classificador de abertura.

/// Nível de confiança do rótulo. `Named` = casou uma heurística
/// nomeada; `Signature` = fallback honesto baseado nos primeiros
/// marcos de supply; `Insufficient` = replay curto demais para
/// classificar (< 3 min de game time).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confidence {
    Named,
    Signature,
    Insufficient,
}

/// Rótulo de abertura pronto para display.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OpeningLabel {
    pub opening: String,
    pub follow_up: Option<String>,
    pub confidence: Confidence,
}

impl OpeningLabel {
    /// Monta a string de display: `"{opening} — {follow_up}"` ou só
    /// `"{opening}"`.
    pub fn to_display_string(&self) -> String {
        match &self.follow_up {
            Some(f) if !f.is_empty() => format!("{} — {}", self.opening, f),
            _ => self.opening.clone(),
        }
    }
}
