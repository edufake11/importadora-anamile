//! Analizador incremental de Server-Sent Events (SSE).
//!
//! Los bytes llegan en trozos arbitrarios (incluso cortando un carácter UTF-8
//! por la mitad), así que acumulamos bytes y sólo decodificamos eventos
//! completos (separados por una línea en blanco).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        // Normaliza CRLF -> LF a medida que llegan los bytes.
        for &b in chunk {
            if b != b'\r' {
                self.buf.push(b);
            }
        }
        let mut out = Vec::new();
        while let Some(pos) = find_double_newline(&self.buf) {
            let raw: Vec<u8> = self.buf.drain(..pos + 2).collect();
            if let Some(ev) = parse_event(&raw[..pos]) {
                out.push(ev);
            }
        }
        out
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn parse_event(raw: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(raw);
    let mut event = None;
    let mut data_lines: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() || line.starts_with(':') {
            continue; // comentario / keep-alive
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data_lines.push(value),
            _ => {}
        }
    }
    if data_lines.is_empty() && event.is_none() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let mut p = SseParser::new();
        assert!(p.push(b"event: ping\nda").is_empty());
        let evs = p.push(b"ta: {\"a\":1}\n\nevent: x\ndata: 2\n\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("ping"));
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].data, "2");
    }

    #[test]
    fn handles_crlf_and_comments() {
        let mut p = SseParser::new();
        let evs = p.push(b": keep-alive\r\n\r\ndata: hola\r\n\r\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: None,
                data: "hola".into()
            }]
        );
    }

    #[test]
    fn utf8_split_mid_character() {
        let mut p = SseParser::new();
        let msg = "data: ñandú 🐾\n\n".as_bytes();
        let (a, b) = msg.split_at(8); // corta dentro de "ñ"
        assert!(p.push(a).is_empty());
        let evs = p.push(b);
        assert_eq!(evs[0].data, "ñandú 🐾");
    }

    #[test]
    fn multiline_data_is_joined() {
        let mut p = SseParser::new();
        let evs = p.push(b"data: a\ndata: b\n\n");
        assert_eq!(evs[0].data, "a\nb");
    }
}
