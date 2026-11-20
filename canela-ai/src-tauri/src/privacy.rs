//! Detección de información sensible antes de enviar algo fuera del equipo
//! (v0.6.1). No depende sólo de palabras clave: valida los datos con sus
//! reglas matemáticas (Luhn, IBAN mod-97, letra del DNI/NIE…) y mide la
//! aleatoriedad (entropía) de posibles claves.
//!
//! Tres niveles:
//! - **Secreto** (credenciales, tarjetas, cuentas, claves privadas, documentos
//!   de identidad válidos): nunca sale del equipo.
//! - **Personal** (correos, teléfonos, direcciones, salud, documentos sin
//!   validar): sólo sale con tu autorización explícita para ESE mensaje.
//! - **Incierto** (imágenes o capturas, que Canela no puede revisar por
//!   dentro): también requiere autorización explícita.
//!
//! Se revisa todo lo que se enviaría: el mensaje, los adjuntos y el
//! historial de la conversación.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Nada detectado.
    None,
    /// No se puede garantizar (imágenes).
    Uncertain,
    /// Datos personales: con autorización explícita.
    Personal,
    /// Secretos: nunca salen.
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub level: Level,
    /// Identificador estable (para la autorización): «tarjeta», «correo»…
    pub kind: String,
    /// Descripción para el usuario: «un número de tarjeta».
    pub what: String,
    /// Dónde: «mensaje», «adjunto», «historial», «imagen».
    pub place: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn level(&self) -> Level {
        self.findings.iter().map(|f| f.level).max().unwrap_or(Level::None)
    }

    pub fn kinds_needing_ack(&self) -> Vec<String> {
        let mut k: Vec<String> = self
            .findings
            .iter()
            .filter(|f| matches!(f.level, Level::Personal | Level::Uncertain))
            .map(|f| f.kind.clone())
            .collect();
        k.sort();
        k.dedup();
        k
    }

    /// Frase corta: «un número de tarjeta (mensaje) y un correo (historial)».
    pub fn summary(&self) -> String {
        let mut seen = Vec::new();
        for f in &self.findings {
            let s = format!("{} ({})", f.what, f.place);
            if !seen.contains(&s) {
                seen.push(s);
            }
        }
        match seen.len() {
            0 => String::new(),
            1 => seen[0].clone(),
            _ => {
                let last = seen.pop().unwrap();
                format!("{} y {last}", seen.join(", "))
            }
        }
    }
}

// ------------------------------------------------------------ validadores

/// Algoritmo de Luhn (tarjetas de crédito y débito).
pub fn luhn(digits: &str) -> bool {
    let d: Vec<u32> = digits.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&d.len()) || d.iter().all(|x| *x == d[0]) {
        return false;
    }
    let sum: u32 = d
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| {
            if i % 2 == 1 {
                let y = x * 2;
                if y > 9 {
                    y - 9
                } else {
                    y
                }
            } else {
                x
            }
        })
        .sum();
    sum % 10 == 0
}

/// IBAN con su dígito de control (mod 97).
pub fn iban_valid(s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if !(15..=34).contains(&s.len()) || !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let b = s.as_bytes();
    if !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() || !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return false;
    }
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u64 = 0;
    for c in rearranged.chars() {
        let v = if c.is_ascii_digit() {
            c as u64 - '0' as u64
        } else {
            c as u64 - 'A' as u64 + 10
        };
        rem = if v >= 10 { (rem * 100 + v) % 97 } else { (rem * 10 + v) % 97 };
    }
    rem == 1
}

/// DNI o NIE español con su letra de control.
pub fn dni_nie_valid(s: &str) -> bool {
    let s = s.to_uppercase();
    if s.len() != 9 {
        return false;
    }
    let (body, letter) = s.split_at(8);
    let body = match body.chars().next() {
        Some('X') => format!("0{}", &body[1..]),
        Some('Y') => format!("1{}", &body[1..]),
        Some('Z') => format!("2{}", &body[1..]),
        _ => body.to_string(),
    };
    let Ok(n) = body.parse::<u32>() else { return false };
    const LETTERS: &[u8] = b"TRWAGMYFPDXBNJZSQVHLCKE";
    LETTERS[(n % 23) as usize] as char == letter.chars().next().unwrap_or(' ')
}

/// Formato de CURP mexicana (18 caracteres).
pub fn curp_like(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 18
        && b[..4].iter().all(u8::is_ascii_uppercase)
        && b[4..10].iter().all(u8::is_ascii_digit)
        && (b[10] == b'H' || b[10] == b'M' || b[10] == b'X')
        && b[11..16].iter().all(u8::is_ascii_uppercase)
        && b[16].is_ascii_alphanumeric()
        && b[17].is_ascii_digit()
}

/// Cadena hexadecimal larga: puede ser una clave o sólo un hash (commit,
/// suma de comprobación). Como no se puede saber, se pide autorización.
pub fn long_hex(w: &str) -> bool {
    let w = w.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    w.len() >= 32
        && w.chars().all(|c| c.is_ascii_hexdigit())
        && w.chars().any(|c| c.is_ascii_digit())
        && w.chars().any(|c| c.is_ascii_alphabetic())
}

/// Aleatoriedad de Shannon por carácter.
pub fn entropy(s: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    let n = s.chars().count() as f64;
    for c in s.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    counts
        .values()
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

/// ¿Parece una clave o token generado (largo, mezcla de clases, aleatorio)?
pub fn looks_like_secret_token(w: &str) -> bool {
    let w = w.trim_matches(|c: char| "\"'`,;()[]{}<>".contains(c));
    if w.len() < 24 || w.len() > 200 || w.contains("://") || w.contains('/') && w.matches('/').count() > 1 {
        return false;
    }
    if !w.chars().all(|c| c.is_ascii_alphanumeric() || "-_+=.".contains(c)) {
        return false;
    }
    let lower = w.chars().any(|c| c.is_ascii_lowercase());
    let upper = w.chars().any(|c| c.is_ascii_uppercase());
    let digit = w.chars().any(|c| c.is_ascii_digit());
    let classes = [lower, upper, digit].iter().filter(|x| **x).count();
    // Las claves generadas alternan mucho entre minúsculas, mayúsculas y
    // cifras; las palabras normales (aunque vayan juntas) casi no.
    let class = |c: char| {
        if c.is_ascii_lowercase() {
            0
        } else if c.is_ascii_uppercase() {
            1
        } else if c.is_ascii_digit() {
            2
        } else {
            3
        }
    };
    let chars: Vec<char> = w.chars().collect();
    let switches = chars.windows(2).filter(|p| class(p[0]) != class(p[1])).count();
    let switch_ratio = switches as f64 / (chars.len() - 1) as f64;
    classes >= 3 && entropy(w) >= 3.8 && switch_ratio >= 0.3
}

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            _ => c,
        })
        .collect()
}

// ------------------------------------------------------------ análisis

fn push(out: &mut Vec<Finding>, level: Level, kind: &str, what: &str, place: &str) {
    if !out.iter().any(|f| f.kind == kind && f.place == place) {
        out.push(Finding {
            level,
            kind: kind.into(),
            what: what.into(),
            place: place.into(),
        });
    }
}

/// Analiza un texto. `place` dice de dónde viene (mensaje, adjunto, historial).
pub fn scan_text(text: &str, place: &str, out: &mut Vec<Finding>) {
    use Level::{Personal, Secret};
    let f = fold(text);

    // --- Secretos con estructura inequívoca
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        push(out, Secret, "clave_privada", "una clave privada", place);
    }
    // Palabras clave de la v0.2 (las secuencias de cifras se validan abajo con Luhn).
    if let Some(r) = crate::db::looks_sensitive(text).filter(|r| !r.contains("número de tarjeta")) {
        let kind = match r {
            "una contraseña" | "un PIN" => "contrasena",
            "datos de tarjeta" => "tarjeta",
            "una clave de API" | "un token" => "clave_api",
            _ => "identificacion",
        };
        push(out, Secret, kind, r, place);
    }
    // Asignaciones tipo .env / configuración: PASSWORD=..., api_key: ...
    for line in text.lines() {
        let l = line.trim();
        if let Some(pos) = l.find(['=', ':']) {
            let key = fold(&l[..pos]).replace(['"', '\'', ' '], "");
            let val = l[pos + 1..].trim().trim_matches(['"', '\'']);
            let secret_key = [
                "password",
                "passwd",
                "pwd",
                "secret",
                "token",
                "apikey",
                "api_key",
                "contrasena",
                "clave",
            ]
            .iter()
            .any(|k| key.ends_with(k) || key.contains(&format!("{k}_")) || key == *k);
            if secret_key && val.len() >= 4 && !val.contains(' ') && key.len() <= 40 {
                push(
                    out,
                    Secret,
                    "contrasena",
                    "una contraseña o clave en un archivo de configuración",
                    place,
                );
            }
        }
    }
    // Cadenas de conexión con usuario:contraseña@host
    for w in text.split_whitespace() {
        if let Some(rest) = w.split("://").nth(1) {
            if let Some(cred) = rest.split('@').next().filter(|_| rest.contains('@')) {
                if cred.contains(':') && !cred.ends_with(':') {
                    push(
                        out,
                        Secret,
                        "contrasena",
                        "una contraseña dentro de una dirección de conexión",
                        place,
                    );
                }
            }
        }
        // JWT: tres partes base64url, la primera empieza por eyJ.
        if w.starts_with("eyJ") && w.matches('.').count() == 2 && w.len() > 40 {
            push(out, Secret, "clave_api", "un token de acceso (JWT)", place);
        } else if looks_like_secret_token(w) {
            push(out, Secret, "clave_api", "algo que parece una clave o token", place);
        } else if long_hex(w) {
            push(out, Personal, "hex", "un código hexadecimal largo (¿clave o hash?)", place);
        }
    }

    // --- Números validados
    let (mut run, mut digits) = (0usize, String::new());
    let check_digits = |digits: &str, out: &mut Vec<Finding>| {
        if luhn(digits) {
            push(out, Secret, "tarjeta", "un número de tarjeta", place);
        } else if (13..=19).contains(&digits.len()) {
            push(out, Personal, "numero_largo", "un número largo (¿cuenta o documento?)", place);
        }
    };
    for c in text.chars().chain(std::iter::once('\n')) {
        if c.is_ascii_digit() {
            run += 1;
            digits.push(c);
        } else if c != ' ' && c != '-' {
            if run >= 13 {
                check_digits(&digits, out);
            }
            run = 0;
            digits.clear();
        }
    }
    for w in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        let up = w.to_uppercase();
        if up.len() >= 15 && iban_valid(&up) {
            push(out, Secret, "cuenta", "un número de cuenta bancaria (IBAN)", place);
        }
        if dni_nie_valid(&up) {
            push(out, Secret, "identificacion", "un DNI/NIE", place);
        }
        if curp_like(w) {
            push(out, Secret, "identificacion", "una CURP", place);
        }
    }
    // IBAN escrito en grupos («ES91 2100 0418…», «DE89 3704…»): de 15 a 34
    // caracteres según el país.
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let cb = compact.as_bytes();
    'iban: for start in 0..cb.len().saturating_sub(15) {
        if !(cb[start].is_ascii_uppercase()
            && cb[start + 1].is_ascii_uppercase()
            && cb[start + 2].is_ascii_digit()
            && cb[start + 3].is_ascii_digit())
        {
            continue;
        }
        for len in 15..=34usize {
            let end = start + len;
            if end > cb.len() || !compact.is_char_boundary(end) {
                break;
            }
            if iban_valid(&compact[start..end]) {
                push(out, Secret, "cuenta", "un número de cuenta bancaria (IBAN)", place);
                break 'iban;
            }
        }
    }
    // SSN de EE. UU.: 123-45-6789
    if text.split_whitespace().any(|w| {
        let p: Vec<&str> = w.trim_matches(|c: char| !c.is_ascii_digit()).split('-').collect();
        p.len() == 3 && p[0].len() == 3 && p[1].len() == 2 && p[2].len() == 4 && p.iter().all(|x| x.chars().all(|c| c.is_ascii_digit()))
    }) {
        push(out, Secret, "identificacion", "un número de seguro social", place);
    }

    // --- Datos personales (con autorización)
    if text.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.');
        let mut p = w.split('@');
        // El dominio termina en letras («.com», «.mx»): «react@18.3.1» no es un correo.
        matches!((p.next(), p.next(), p.next()), (Some(a), Some(b), None)
            if !a.is_empty() && b.contains('.')
                && b.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())))
    }) {
        push(out, Personal, "correo", "una dirección de correo", place);
    }
    // Teléfonos: de 1 a 5 grupos seguidos de cifras (+52 55 1234 5678,
    // 55 4433 2211, 555-123-4567) con 9 a 13 cifras en total.
    let tokens: Vec<&str> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| ",.;:".contains(c)))
        .collect();
    'phone: for end in 0..tokens.len() {
        for start in end.saturating_sub(4)..=end {
            let group = &tokens[start..=end];
            if !group
                .iter()
                .all(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || "+-().".contains(c)))
            {
                continue;
            }
            let n: usize = group.iter().map(|t| t.chars().filter(|c| c.is_ascii_digit()).count()).sum();
            let shaped = group.len() > 1 || group[0].starts_with('+') || group[0].contains('-');
            if (9..=13).contains(&n) && (shaped || n == 10) && !group.iter().any(|t| t.contains('.')) {
                push(out, Personal, "telefono", "un número de teléfono", place);
                break 'phone;
            }
        }
    }
    for (k, kind, what) in [
        ("diagnostico", "salud", "datos de salud"),
        ("historia clinica", "salud", "datos de salud"),
        ("receta medica", "salud", "datos de salud"),
        ("mi enfermedad", "salud", "datos de salud"),
        ("mi direccion es", "direccion", "una dirección"),
        ("vivo en la calle", "direccion", "una dirección"),
        ("codigo postal", "direccion", "una dirección"),
        ("mi salario", "finanzas", "datos financieros"),
        ("mi sueldo", "finanzas", "datos financieros"),
        ("estado de cuenta", "finanzas", "datos financieros"),
        ("pasaporte", "identificacion_sin_validar", "un documento de identidad"),
        ("curp", "identificacion_sin_validar", "un documento de identidad"),
        ("rfc", "identificacion_sin_validar", "un documento de identidad"),
        ("numero de seguro", "identificacion_sin_validar", "un documento de identidad"),
    ] {
        if f.split(|c: char| !c.is_alphanumeric()).collect::<Vec<_>>().join(" ").contains(k) {
            push(out, Personal, kind, what, place);
        }
    }
}

/// Mensaje que se enviaría, para analizar.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Outgoing {
    pub text: String,
    /// Contenido de archivos adjuntos (si viene aparte del texto).
    pub attachments: Vec<String>,
    /// Textos anteriores de la conversación que también se enviarían.
    pub history: Vec<String>,
    /// Imágenes en el mensaje o en el historial.
    pub images: usize,
}

pub fn scan(o: &Outgoing) -> Report {
    let mut out = Vec::new();
    scan_text(&o.text, "mensaje", &mut out);
    for a in &o.attachments {
        scan_text(a, "adjunto", &mut out);
    }
    for h in &o.history {
        scan_text(h, "historial", &mut out);
    }
    if o.images > 0 {
        push(
            &mut out,
            Level::Uncertain,
            "imagen",
            "imágenes o capturas que no puedo revisar por dentro",
            "imagen",
        );
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.level));
    Report { findings: out }
}

/// Si el texto (p. ej. un archivo leído por una herramienta) contiene
/// secretos, devuelve un aviso para enviar EN SU LUGAR a un servicio externo.
pub fn redact_for_external(text: &str, service: &str) -> Option<String> {
    let mut f = Vec::new();
    scan_text(text, "archivo", &mut f);
    let secrets: Vec<String> = f.iter().filter(|x| x.level == Level::Secret).map(|x| x.what.clone()).collect();
    if secrets.is_empty() {
        return None;
    }
    Some(format!(
        "[Contenido omitido por privacidad: contiene {}. No se envía a {service}. Dile al usuario qué archivo era y que, si quiere analizarlo, use el modelo local o quite esos datos.]",
        secrets.join(", ")
    ))
}

/// Control final antes de enviar a un servicio externo.
/// `acks` = tipos de datos que autorizaste explícitamente para este envío.
pub fn check_external(report: &Report, acks: &[String], service: &str) -> Result<(), String> {
    if report.level() == Level::Secret {
        let secrets: Vec<&Finding> = report.findings.iter().filter(|f| f.level == Level::Secret).collect();
        let what: Vec<String> = secrets.iter().map(|f| format!("{} ({})", f.what, f.place)).collect();
        return Err(format!(
            "Por tu privacidad no enviaré esto a {service}: contiene {}. Quítalo (o empieza una conversación nueva si está en el historial) o usa el modelo local.",
            what.join(", ")
        ));
    }
    let missing: Vec<String> = report.kinds_needing_ack().into_iter().filter(|k| !acks.contains(k)).collect();
    if !missing.is_empty() {
        return Err(format!(
            "Antes de enviar esto a {service} necesito tu autorización: contiene {}.",
            report.summary()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(t: &str) -> Level {
        scan(&Outgoing {
            text: t.into(),
            ..Default::default()
        })
        .level()
    }

    #[test]
    fn validators() {
        assert!(luhn("4111111111111111"));
        assert!(luhn("5500 0000 0000 0004"));
        assert!(!luhn("4111111111111112"));
        assert!(!luhn("1111111111111111"), "repetidos no cuentan");
        assert!(iban_valid("ES9121000418450200051332"));
        assert!(iban_valid("GB82 WEST 1234 5698 7654 32"));
        assert!(!iban_valid("ES9121000418450200051333"));
        assert!(dni_nie_valid("12345678Z"));
        assert!(!dni_nie_valid("12345678A"));
        assert!(dni_nie_valid("X1234567L"));
        assert!(curp_like("GODE561231HDFRRN09"));
        assert!(!curp_like("HOLA"));
        assert!(entropy("aaaa") < 0.1);
        assert!(looks_like_secret_token("sk9Qe7XkLm2Pw4Rt8Vb1Nc6Zd3Hf5Jg0"));
        assert!(
            !looks_like_secret_token("ProgramacionOrientadaAObjetos2026"),
            "palabras normales con mayúsculas no bastan: {}",
            entropy("ProgramacionOrientadaAObjetos2026")
        );
        assert!(!looks_like_secret_token("https://es.wikipedia.org/wiki/Rust_(lenguaje)"));
    }

    #[test]
    fn secrets_are_detected_without_keywords() {
        // Sin decir «tarjeta» ni «contraseña»: lo detecta por su forma.
        assert_eq!(level("cóbrame en 4111 1111 1111 1111 vence 12/28"), Level::Secret);
        assert_eq!(level("transfiere a ES91 2100 0418 4502 0005 1332"), Level::Secret);
        assert_eq!(level("mi documento es 12345678Z"), Level::Secret);
        assert_eq!(level("DATABASE_URL=postgres://admin:Sup3rS3cret@db.local:5432/app"), Level::Secret);
        assert_eq!(level("DB_PASSWORD=Perro123"), Level::Secret);
        assert_eq!(
            level("usa este token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N"),
            Level::Secret
        );
        assert_eq!(level("la clave del servidor es sk9Qe7XkLm2Pw4Rt8Vb1Nc6Zd3Hf5Jg0"), Level::Secret);
        assert_eq!(
            level("-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAA\n-----END"),
            Level::Secret
        );
        assert_eq!(level("mi seguro social es 123-45-6789"), Level::Secret);
    }

    #[test]
    fn personal_data_needs_authorization() {
        assert_eq!(level("escríbele a ana.lopez@gmail.com"), Level::Personal);
        assert_eq!(level("llámame al +52 55 1234 5678"), Level::Personal);
        assert_eq!(level("mi diagnóstico de diabetes salió ayer"), Level::Personal);
        assert_eq!(level("número 1234567890123 que no es tarjeta"), Level::Personal);
    }

    #[test]
    fn ordinary_text_is_not_flagged() {
        for t in [
            "explícame cómo funciona React",
            "tengo 3 tareas para el 2026 y 15 archivos",
            "compara Python 3.12 con Rust 1.82",
            "el commit 582f23f arregló el error",
            "abre https://es.wikipedia.org/wiki/Perro_salchicha",
            "el resultado es 3.14159 y la versión 0.6.0",
            "ProgramacionOrientadaAObjetos es un tema largo",
        ] {
            assert_eq!(level(t), Level::None, "{t}");
        }
    }

    #[test]
    fn attachments_history_and_images_are_checked() {
        let r = scan(&Outgoing {
            text: "resume este archivo".into(),
            attachments: vec!["API_KEY=abc123XYZ789secretvalue".into()],
            ..Default::default()
        });
        assert_eq!(r.level(), Level::Secret);
        assert_eq!(r.findings[0].place, "adjunto");
        let r = scan(&Outgoing {
            text: "¿y qué opinas?".into(),
            history: vec!["mi correo es yo@ejemplo.com".into()],
            ..Default::default()
        });
        assert_eq!(r.level(), Level::Personal);
        assert!(r.summary().contains("historial"));
        let r = scan(&Outgoing {
            text: "¿qué ves?".into(),
            images: 1,
            ..Default::default()
        });
        assert_eq!(r.level(), Level::Uncertain);
    }

    #[test]
    fn external_check_requires_matching_authorization() {
        let r = scan(&Outgoing {
            text: "escríbele a ana@ejemplo.com".into(),
            images: 1,
            ..Default::default()
        });
        assert!(check_external(&r, &[], "Claude").unwrap_err().contains("autorización"));
        assert!(
            check_external(&r, &["correo".into()], "Claude").is_err(),
            "falta autorizar la imagen"
        );
        assert!(check_external(&r, &["correo".into(), "imagen".into()], "Claude").is_ok());
        let s = scan(&Outgoing {
            text: "DB_PASSWORD=Perro123".into(),
            ..Default::default()
        });
        assert!(
            check_external(&s, &["contrasena".into()], "Claude")
                .unwrap_err()
                .contains("no enviaré"),
            "los secretos no se pueden autorizar"
        );
        assert!(check_external(&Report::default(), &[], "Claude").is_ok());
    }

    /// Evaluación con mensajes NUEVOS (no usados para escribir las reglas):
    /// mide aciertos, falsos positivos y falsos negativos.
    #[test]
    fn evaluation_on_fresh_messages() {
        use Level::*;
        let cases: &[(&str, Level)] = &[
            // Secretos
            ("págame con la 5555 5555 5555 4444 por favor", Secret),
            ("número de la visa: 4012888888881881", Secret),
            ("la cuenta para la transferencia es DE89 3704 0044 0532 0130 00", Secret),
            ("NIE Y1234567X para el trámite", Secret),
            ("smtp_password: Gat0Negro#2026", Secret),
            ("export GITHUB_TOKEN=ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8", Secret),
            ("mongodb://root:hunter22@cluster0.mongodb.net/test", Secret),
            ("mi contraseña del correo es girasol88", Secret),
            ("token: Xq9LmP2wRt7VbN4cZd8HfJ3gK6sA1eYu", Secret),
            ("mi CURP es LOMP900215MDFPRL08", Secret),
            // Personales
            ("manda el informe a jefe.ventas@empresa.mx", Personal),
            ("mi número es 55 4433 2211", Personal),
            ("vivo en la calle Reforma 222", Personal),
            ("mi sueldo es de 20 mil al mes, ¿cuánto ahorro?", Personal),
            ("tengo una receta médica de amoxicilina", Personal),
            ("revisa el commit 3f786850e387550fdab836ed7e6dc881de23001b", Personal),
            // Nada sensible
            ("¿cuántos gramos hay en 2.5 kilos?", None),
            ("instala react@18.3.1 con npm", None),
            ("el error está en la línea 1234 del archivo main.rs", None),
            ("pon la alarma a las 07:30", None),
            ("compara el iPhone 15 con el Pixel 9", None),
            ("¿qué es un hash SHA-256?", None),
            ("la factura F-2026-0042 vence el 30/11", None),
            ("mi perro se llama Canela y tiene 3 años", None),
            ("ejecuta cargo test --release en el proyecto", None),
            ("la contraseña debe tener 12 caracteres, ¿qué política recomiendas?", Secret),
            ("UseTheForceLuke es un buen nombre de rama?", None),
            ("la temperatura subió a 38.5 grados hoy", None),
            ("envía el archivo reporte_final_v2.pdf", None),
            ("¿cómo se calcula el dígito de control de un IBAN?", None),
        ];
        let (mut ok, mut fp, mut fneg) = (0, 0, 0);
        for (t, expected) in cases {
            let got = level(t);
            if got == *expected {
                ok += 1;
            } else if got > *expected {
                fp += 1;
                eprintln!("  falso positivo: {t:?}: esperado {expected:?}, obtenido {got:?}");
            } else {
                fneg += 1;
                eprintln!("  FALSO NEGATIVO: {t:?}: esperado {expected:?}, obtenido {got:?}");
            }
        }
        eprintln!(
            "Privacidad con mensajes nuevos: {ok}/{} aciertos, {fp} falsos positivos (pide permiso de más), {fneg} falsos negativos (deja pasar algo)",
            cases.len()
        );
        // Lo grave es dejar pasar un secreto: ningún secreto puede quedar por debajo de «Personal».
        for (t, expected) in cases {
            if *expected == Secret {
                assert!(level(t) >= Personal, "secreto sin detectar: {t}");
            }
        }
        assert!(ok * 100 / cases.len() >= 80, "{ok}/{}", cases.len());
    }

    /// Segundo conjunto nuevo, escrito DESPUÉS de corregir los detectores con
    /// el primero (para que la medida siga siendo honesta).
    #[test]
    fn evaluation_on_second_fresh_set() {
        use Level::*;
        let cases: &[(&str, Level)] = &[
            ("cárgalo a la tarjeta 3782 822463 10005", Secret),
            ("IBAN: FR14 2004 1010 0505 0001 3M02 606", Secret),
            ("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", Secret),
            ("redis://default:Contr4s3na@localhost:6379", Secret),
            ("mi pin de la tarjeta es 4821", Secret),
            ("DNI 87654321X", Secret),
            ("escríbeme a maria_g@outlook.es mañana", Personal),
            ("mi celular: 044 55 1234 5678", Personal),
            ("estado de cuenta de octubre adjunto", Personal),
            ("mi código postal es 06700", Personal),
            ("¿cuánto es 1024 * 768?", None),
            ("descarga node v20.11.1 para windows", None),
            ("el vuelo IB6402 sale a las 23:55", None),
            ("pásame la versión 2.0.0-beta.3 del paquete", None),
            ("la reunión es en la sala 402 del piso 4", None),
            ("¿cómo protejo mis contraseñas con un gestor?", Secret),
            ("mi gato pesa 4.2 kg", None),
            ("abre el puerto 8080 en el firewall", None),
            ("¿qué diferencia hay entre @Component y @Service en Spring?", None),
            ("el pedido #4471-2026 llegó dañado", None),
        ];
        let (mut ok, mut fp, mut fneg) = (0, 0, 0);
        for (t, expected) in cases {
            let got = level(t);
            if got == *expected {
                ok += 1;
            } else if got > *expected {
                fp += 1;
                eprintln!("  falso positivo: {t:?}: esperado {expected:?}, obtenido {got:?}");
            } else {
                fneg += 1;
                eprintln!("  FALSO NEGATIVO: {t:?}: esperado {expected:?}, obtenido {got:?}");
            }
        }
        eprintln!(
            "Privacidad, segundo conjunto nuevo: {ok}/{} aciertos, {fp} falsos positivos, {fneg} falsos negativos",
            cases.len()
        );
        assert!(ok * 100 / cases.len() >= 75, "{ok}/{}", cases.len());
    }
}
