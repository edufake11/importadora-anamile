//! Herramientas de investigación (v0.7): Wikipedia y lectura de páginas.
//!
//! - Sólo http(s) públicos: se rechazan direcciones locales o privadas
//!   (localhost, 127.x, 10.x, 192.168.x, 172.16–31.x, ::1, fc00::/7…), para
//!   que una página no pueda hacer que Canela consulte tu red.
//! - Límite de tamaño (2 MB) y de tiempo (15 s); el texto se recorta.
//! - El contenido es EXTERNO: se entrega envuelto para que el modelo no siga
//!   instrucciones que aparezcan dentro.

use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;

const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TEXT: usize = 20_000;

/// Valida una URL pública. Devuelve la URL normalizada.
pub fn public_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| format!("«{raw}» no es una dirección web válida."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Sólo puedo leer páginas http o https.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("No leo direcciones con usuario o contraseña.".into());
    }
    let host = url.host_str().ok_or("La dirección no tiene servidor.")?.to_lowercase();
    let private = |ip: IpAddr| match ip {
        IpAddr::V4(v) => {
            v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.octets()[0] == 0
        }
        IpAddr::V6(v) => {
            v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80
        }
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal") {
        return Err("Por seguridad no leo direcciones de tu red local.".into());
    }
    if let Ok(ip) = bare.parse::<IpAddr>() {
        if private(ip) {
            return Err("Por seguridad no leo direcciones de tu red local.".into());
        }
    }
    Ok(url)
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').filter(|&e| e <= 10);
        let Some(end) = end else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// HTML → texto legible (sin scripts, estilos ni etiquetas).
pub fn html_to_text(html: &str) -> (Option<String>, String) {
    let lower = html.to_lowercase();
    let title = lower.find("<title").and_then(|i| {
        let start = lower[i..].find('>')? + i + 1;
        let end = lower[start..].find("</title>")? + start;
        Some(decode_entities(html[start..end].trim()))
    });
    let mut text = String::new();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] == b'<' {
            let rest_lower = &lower[i..];
            // Bloques que se descartan enteros.
            let mut skipped = false;
            for (open, close) in [
                ("<script", "</script>"),
                ("<style", "</style>"),
                ("<noscript", "</noscript>"),
                ("<!--", "-->"),
            ] {
                if rest_lower.starts_with(open) {
                    i = rest_lower.find(close).map(|e| i + e + close.len()).unwrap_or(html.len());
                    skipped = true;
                    break;
                }
            }
            if skipped {
                continue;
            }
            let end = rest_lower.find('>').map(|e| i + e + 1).unwrap_or(html.len());
            let tag = &rest_lower[1..(end - i).saturating_sub(1).max(1)];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if [
                "p", "br", "div", "li", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "section", "article", "ul", "ol", "table",
            ]
            .contains(&name.as_str())
            {
                text.push('\n');
            }
            i = end;
        } else {
            let next = html[i..].find('<').map(|n| i + n).unwrap_or(html.len());
            text.push_str(&html[i..next]);
            i = next;
        }
    }
    let decoded = decode_entities(&text);
    let mut clean = String::new();
    for line in decoded.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !l.is_empty() {
            clean.push_str(&l);
            clean.push('\n');
        }
    }
    (title, clean)
}

fn wiki_base(lang: &str) -> String {
    #[cfg(test)]
    if let Ok(u) = std::env::var("CANELA_TEST_WIKI_URL") {
        return u;
    }
    format!("https://{lang}.wikipedia.org")
}

/// Busca en Wikipedia (API pública). Devuelve texto con títulos, extracto y URL.
pub async fn wikipedia(http: &reqwest::Client, query: &str, lang: &str) -> Result<String, String> {
    let lang = if lang.len() == 2 && lang.chars().all(|c| c.is_ascii_lowercase()) {
        lang
    } else {
        "es"
    };
    let base = wiki_base(lang);
    let resp = http
        .get(format!("{base}/w/api.php"))
        .query(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("format", "json"),
            ("srlimit", "5"),
            ("utf8", "1"),
        ])
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("No pude consultar Wikipedia: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Wikipedia respondió {}.", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| format!("Respuesta de Wikipedia no válida: {e}"))?;
    let results = v["query"]["search"].as_array().cloned().unwrap_or_default();
    if results.is_empty() {
        return Ok(format!("Wikipedia ({lang}) no tiene resultados para «{query}»."));
    }
    let site = format!("https://{lang}.wikipedia.org/wiki/");
    let lines: Vec<String> = results
        .iter()
        .filter_map(|r| {
            let title = r["title"].as_str()?;
            let (_, snippet) = html_to_text(r["snippet"].as_str().unwrap_or(""));
            Some(format!(
                "- {title}: {}\n  Fuente: {site}{}",
                snippet.trim().replace('\n', " "),
                title.replace(' ', "_")
            ))
        })
        .collect();
    Ok(format!("Resultados de Wikipedia ({lang}) para «{query}»:\n{}", lines.join("\n")))
}

/// Lee una página pública y devuelve (título, texto recortado).
pub async fn read_page(http: &reqwest::Client, raw: &str) -> Result<String, String> {
    let url = public_url(raw)?;
    let mut resp = http
        .get(url.clone())
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("No pude abrir la página: {e}"))?;
    // Tras redirecciones, la dirección final también debe ser pública.
    public_url(resp.url().as_str())?;
    if !resp.status().is_success() {
        return Err(format!("La página respondió {}.", resp.status()));
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !(ctype.is_empty() || ctype.contains("text/") || ctype.contains("json") || ctype.contains("xml")) {
        return Err(format!("La página no es texto ({ctype})."));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Se cortó la descarga: {e}"))? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            body.truncate(MAX_BYTES);
            break;
        }
    }
    let raw_text = String::from_utf8_lossy(&body).to_string();
    let (title, text) = if ctype.contains("html") || raw_text.trim_start().starts_with('<') {
        html_to_text(&raw_text)
    } else {
        (None, raw_text)
    };
    let total = text.chars().count();
    let cut: String = text.chars().take(MAX_TEXT).collect();
    let final_url = resp.url().to_string();
    Ok(format!(
        "Fuente: {final_url}\nTítulo: {}\n{}{}",
        title.unwrap_or_else(|| "(sin título)".into()),
        cut,
        if total > MAX_TEXT {
            format!("\n[… recortado: {MAX_TEXT} de {total} caracteres]")
        } else {
            String::new()
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_urls() {
        assert!(public_url("https://es.wikipedia.org/wiki/Teckel").is_ok());
        for bad in [
            "http://localhost:8080/admin",
            "http://127.0.0.1/",
            "http://10.0.0.5/",
            "http://192.168.1.1/",
            "http://172.16.0.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://router.local/",
            "file:///etc/passwd",
            "ftp://example.com/x",
            "https://user:pw@example.com/",
            "no es una url",
        ] {
            assert!(public_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn html_is_converted_to_clean_text() {
        let html = "<html><head><title>Perro &amp; gato</title><style>body{color:red}</style><script>alert('x')</script></head>\
<body><h1>Teckel</h1><p>Es un perro <b>alargado</b>.</p><!-- comentario --><ul><li>Orejas largas</li><li>Patas cortas &#233;</li></ul></body></html>";
        let (title, text) = html_to_text(html);
        assert_eq!(title.as_deref(), Some("Perro & gato"));
        assert!(text.contains("Teckel\n"));
        assert!(text.contains("Es un perro alargado."));
        assert!(text.contains("Patas cortas é"));
        assert!(!text.contains("alert") && !text.contains("color:red") && !text.contains("comentario"));
    }

    #[tokio::test]
    async fn wikipedia_search_returns_sources() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            assert!(req.contains("srsearch=Teckel"), "{req}");
            let body = r#"{"query":{"search":[{"title":"Teckel","snippet":"El <span class=\"searchmatch\">teckel</span> es una raza de perro"}]}}"#;
            let _ = sock
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
        });
        std::env::set_var("CANELA_TEST_WIKI_URL", format!("http://{addr}"));
        let out = wikipedia(&reqwest::Client::new(), "Teckel", "es").await.unwrap();
        std::env::remove_var("CANELA_TEST_WIKI_URL");
        assert!(out.contains("Teckel: El teckel es una raza de perro"), "{out}");
        assert!(out.contains("Fuente: https://es.wikipedia.org/wiki/Teckel"));
    }
}
