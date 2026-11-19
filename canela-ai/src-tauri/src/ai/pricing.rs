//! Costos estimados y capacidades conocidas de los modelos.
//!
//! Los precios son APROXIMADOS (USD por millón de tokens) y pueden cambiar:
//! sirven para darte una idea, no para facturar. Claude: tabla oficial de
//! noviembre de 2026. OpenAI: valores publicados conocidos; verifícalos en
//! la página de precios de OpenAI.

use crate::settings::ProviderId;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Tokens de entrada sin caché.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

pub fn price(provider: ProviderId, model: &str) -> Option<Price> {
    let m = model.to_lowercase();
    let p = |input: f64, output: f64, cache_read: f64| Some(Price { input, output, cache_read });
    match provider {
        // Modelos locales: no hay costo por uso (sólo la electricidad).
        ProviderId::Ollama => p(0.0, 0.0, 0.0),
        ProviderId::Anthropic => match m.as_str() {
            "claude-fable-5-2" | "claude-mythos-5-2" => p(8.0, 40.0, 0.20),
            "claude-fable-5-1" | "claude-mythos-5-1" => p(10.0, 50.0, 0.25),
            "claude-fable-5" | "claude-mythos-5" => p(10.0, 50.0, 1.0),
            "claude-opus-5-5" => p(4.0, 20.0, 0.20),
            "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => p(5.0, 25.0, 0.50),
            "claude-sonnet-5-5" => p(2.0, 10.0, 0.10),
            "claude-sonnet-5" => p(2.0, 10.0, 0.20),
            "claude-sonnet-4-6" => p(3.0, 15.0, 0.30),
            "claude-haiku-5-5" => p(0.10, 0.50, 0.01),
            _ if m.starts_with("claude-haiku-4-5") => p(1.0, 5.0, 0.10),
            _ => None,
        },
        ProviderId::Openai => {
            if m.starts_with("gpt-5-nano") {
                p(0.05, 0.40, 0.005)
            } else if m.starts_with("gpt-5-mini") {
                p(0.25, 2.0, 0.025)
            } else if m == "gpt-5" || m.starts_with("gpt-5-20") {
                p(1.25, 10.0, 0.125)
            } else if m.starts_with("gpt-4.1-mini") {
                p(0.40, 1.60, 0.10)
            } else if m.starts_with("gpt-4.1") {
                p(2.0, 8.0, 0.50)
            } else if m.starts_with("gpt-4o-mini") {
                p(0.15, 0.60, 0.075)
            } else if m.starts_with("gpt-4o") {
                p(2.50, 10.0, 1.25)
            } else {
                None
            }
        }
    }
}

/// Costo estimado en USD. Escribir en caché cuesta ~1,25× la entrada.
pub fn cost(price: Price, u: &Usage) -> f64 {
    (u.input as f64 * price.input
        + u.cache_write as f64 * price.input * 1.25
        + u.cache_read as f64 * price.cache_read
        + u.output as f64 * price.output)
        / 1_000_000.0
}

/// Oculta claves API que pudieran aparecer en mensajes de error.
pub fn redact_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        let boundary = i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || matches!(chars[i - 1], '-' | '_'));
        if boundary && (rest == "sk-" || rest == "sk_") {
            let mut j = i + 3;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || matches!(chars[j], '-' | '_' | '*' | '.')) {
                j += 1;
            }
            if j - i >= 7 {
                out.push_str("sk-…(clave oculta)");
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs() {
        let p = price(ProviderId::Anthropic, "claude-opus-5-5").unwrap();
        let u = Usage {
            input: 1_000_000,
            output: 100_000,
            cache_read: 0,
            cache_write: 0,
        };
        assert!((cost(p, &u) - 6.0).abs() < 1e-9); // 4 + 2
        let u2 = Usage {
            input: 0,
            output: 0,
            cache_read: 1_000_000,
            cache_write: 0,
        };
        assert!((cost(p, &u2) - 0.20).abs() < 1e-9);
        assert!(price(ProviderId::Openai, "gpt-5").is_some());
        assert!(price(ProviderId::Openai, "modelo-desconocido").is_none());
    }

    #[test]
    fn hides_keys() {
        let e = "Incorrect API key provided: sk-proj-abc123XYZ. Also sk-ant-api03-zzzz****1234 here";
        let r = redact_secrets(e);
        assert!(!r.contains("abc123"));
        assert!(!r.contains("zzzz"));
        assert!(r.contains("(clave oculta)"));
        assert_eq!(redact_secrets("sin claves; task-sk-1"), "sin claves; task-sk-1");
    }
}
