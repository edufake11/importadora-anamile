//! Lectura local de documentos: texto/código, PDF y Word (.docx).
//!
//! Todo se procesa en tu equipo. Los documentos largos se dividen en partes
//! para no enviar (ni cargar en la conversación) más de lo necesario.

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const PART_CHARS: usize = 12_000;
const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;
const MAX_DOC_BYTES: u64 = 40 * 1024 * 1024;
const MAX_XML_BYTES: u64 = 30 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Text,
    Pdf,
    Docx,
    Image,
    Unsupported,
}

pub fn kind_of(path: &Path) -> DocKind {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => DocKind::Pdf,
        "docx" => DocKind::Docx,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => DocKind::Image,
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "yaml" | "yml" | "toml" | "xml" | "html" | "htm" | "css" | "scss"
        | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "py" | "rs" | "go" | "java" | "kt" | "c" | "h" | "cpp" | "hpp" | "cs" | "php"
        | "rb" | "swift" | "sql" | "sh" | "ini" | "cfg" | "vue" | "svelte" | "dart" | "lua" | "r" | "ipynb" | "gradle" | "env"
        | "gitignore" | "dockerfile" => DocKind::Text,
        _ if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| ["Dockerfile", "Makefile", "README", "LICENSE"].contains(&n)) =>
        {
            DocKind::Text
        }
        _ => DocKind::Unsupported,
    }
}

#[derive(Debug, Clone)]
pub struct Extracted {
    pub text: String,
    /// Páginas (PDF) si se conocen.
    pub pages: Option<usize>,
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Extrae el texto de `word/document.xml` (párrafos, tabulaciones, saltos).
pub fn docx_text_from_xml(xml: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let b = xml.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let end = match xml[i..].find('>') {
                Some(e) => i + e,
                None => break,
            };
            let tag = &xml[i + 1..end];
            let name = tag.split([' ', '/']).next().unwrap_or("");
            match name {
                "w:t" if !tag.ends_with('/') => {
                    if let Some(close) = xml[end + 1..].find("</w:t>") {
                        out.push_str(&decode_entities(&xml[end + 1..end + 1 + close]));
                        i = end + 1 + close + 6;
                        continue;
                    }
                }
                "w:tab" => out.push('\t'),
                "w:br" | "w:cr" => out.push('\n'),
                _ if tag == "/w:p" => out.push('\n'),
                _ => {}
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

fn read_docx(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("No pude abrir el documento: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "El archivo .docx está dañado o no es un documento de Word.".to_string())?;
    let entry = zip
        .by_name("word/document.xml")
        .map_err(|_| "El documento de Word no tiene contenido legible.".to_string())?;
    if entry.size() > MAX_XML_BYTES {
        return Err("El documento de Word es demasiado grande.".into());
    }
    let mut xml = String::new();
    entry
        .take(MAX_XML_BYTES)
        .read_to_string(&mut xml)
        .map_err(|e| format!("No pude leer el documento: {e}"))?;
    Ok(docx_text_from_xml(&xml))
}

/// Devuelve (texto con rótulos de página, nº de páginas, letras/números reales).
fn read_pdf(path: &Path) -> Result<(String, usize, usize), String> {
    let p = path.to_path_buf();
    // Algunos PDF raros hacen fallar la librería: se aísla el fallo.
    let pages = std::panic::catch_unwind(move || pdf_extract::extract_text_by_pages(&p))
        .map_err(|_| "No pude leer este PDF (formato no compatible).".to_string())?
        .map_err(|e| format!("No pude leer el PDF: {e}"))?;
    let n = pages.len();
    let real = pages.iter().map(|t| t.chars().filter(|c| c.is_alphanumeric()).count()).sum();
    let text = pages
        .iter()
        .enumerate()
        .map(|(i, t)| format!("[Página {}]\n{}", i + 1, t.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok((text, n, real))
}

fn read_text(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("No pude abrir el archivo: {e}"))?;
    let mut buf = Vec::new();
    file.take(MAX_TEXT_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| format!("No pude leer el archivo: {e}"))?;
    if buf.iter().take(8192).any(|b| *b == 0) {
        return Err("Es un archivo binario; no lo puedo leer como texto.".into());
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Extrae el texto de un documento compatible.
pub fn extract(path: &Path) -> Result<Extracted, String> {
    let md = std::fs::metadata(path).map_err(|e| format!("No encontré el archivo: {e}"))?;
    if md.is_dir() {
        return Err("Es una carpeta, no un documento.".into());
    }
    match kind_of(path) {
        DocKind::Text => Ok(Extracted {
            text: read_text(path)?,
            pages: None,
        }),
        DocKind::Docx => {
            if md.len() > MAX_DOC_BYTES {
                return Err("El documento es demasiado grande (máximo 40 MB).".into());
            }
            Ok(Extracted {
                text: read_docx(path)?,
                pages: None,
            })
        }
        DocKind::Pdf => {
            if md.len() > MAX_DOC_BYTES {
                return Err("El PDF es demasiado grande (máximo 40 MB).".into());
            }
            let (text, pages, real) = read_pdf(path)?;
            if real < pages.max(1) * 5 {
                return Err(
                    "Este PDF parece escaneado (sólo imágenes, sin texto). Ábrelo y compárteme una captura de la página que te interesa."
                        .into(),
                );
            }
            Ok(Extracted { text, pages: Some(pages) })
        }
        DocKind::Image => Err("Es una imagen: usa ver_imagen.".into()),
        DocKind::Unsupported => Err("Ese tipo de archivo no es compatible (texto, código, PDF, Word o imágenes).".into()),
    }
}

/// Divide en partes de ~`size` caracteres respetando párrafos cuando se puede.
pub fn split_parts(text: &str, size: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for para in text.split_inclusive('\n') {
        if cur.chars().count() + para.chars().count() > size && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        if para.chars().count() > size {
            // Párrafo enorme: se corta por caracteres.
            let chars: Vec<char> = para.chars().collect();
            for c in chars.chunks(size) {
                parts.push(c.iter().collect());
            }
            continue;
        }
        cur.push_str(para);
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    if parts.is_empty() {
        parts.push(String::new());
    }
    parts
}

#[cfg(test)]
pub mod tests_support {
    use std::io::Write;
    use std::path::Path;

    /// Crea un .docx mínimo válido para pruebas.
    pub fn make_docx(path: &Path, paragraphs: &[&str]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let body: String = paragraphs
            .iter()
            .map(|p| format!("<w:p><w:r><w:t xml:space=\"preserve\">{p}</w:t></w:r></w:p>"))
            .collect();
        z.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        write!(z, "<?xml version=\"1.0\"?><w:document><w:body>{body}</w:body></w:document>").unwrap();
        z.finish().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::make_docx;
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("canela-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn docx_xml_parsing() {
        let xml = r#"<w:p><w:r><w:t>Hola</w:t></w:r><w:r><w:tab/><w:t xml:space="preserve"> mundo &amp; más</w:t></w:r></w:p><w:p><w:r><w:t>Segunda</w:t></w:r></w:p>"#;
        assert_eq!(docx_text_from_xml(xml), "Hola\t mundo & más\nSegunda");
    }

    #[test]
    fn reads_docx_and_text() {
        let d = tmp("read");
        make_docx(&d.join("informe.docx"), &["Informe de ventas", "Total: 42 unidades"]);
        let e = extract(&d.join("informe.docx")).unwrap();
        assert!(e.text.contains("Total: 42 unidades"));
        std::fs::write(d.join("notas.md"), "# Notas\nhola").unwrap();
        assert_eq!(extract(&d.join("notas.md")).unwrap().text, "# Notas\nhola");
        std::fs::write(d.join("x.bin"), [0u8, 1, 2]).unwrap();
        assert!(extract(&d.join("x.bin")).is_err());
        std::fs::write(d.join("roto.docx"), "no es zip").unwrap();
        assert!(extract(&d.join("roto.docx")).is_err());
        std::fs::write(d.join("roto.pdf"), "%PDF-1.4 basura").unwrap();
        assert!(
            extract(&d.join("roto.pdf")).is_err(),
            "un PDF dañado debe dar error, no colgar la app"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// PDF mínimo válido (1 página, Helvetica) con offsets de xref correctos.
    fn make_pdf(path: &Path, lines: &[&str]) {
        let mut content = String::from("BT /F1 18 Tf 72 720 Td 22 TL\n");
        for l in lines {
            content.push_str(&format!("({l}) Tj T*\n"));
        }
        content.push_str("ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        ];
        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
        }
        let xref = out.len();
        out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
        for off in offsets {
            out.push_str(&format!("{off:010} 00000 n \n"));
        }
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        ));
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn reads_real_pdf_text() {
        let d = tmp("pdf");
        make_pdf(&d.join("factura.pdf"), &["Factura numero 2026-118", "Total a pagar: 1500 pesos"]);
        let e = extract(&d.join("factura.pdf")).unwrap();
        assert_eq!(e.pages, Some(1));
        assert!(e.text.contains("Factura numero 2026-118"), "{}", e.text);
        assert!(e.text.contains("Total a pagar"), "{}", e.text);
        // Un PDF sin texto (escaneado) da un mensaje claro.
        make_pdf(&d.join("escaneado.pdf"), &[]);
        assert!(extract(&d.join("escaneado.pdf")).unwrap_err().contains("escaneado"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn splits_into_parts() {
        let text = (0..100).map(|i| format!("Párrafo {i} con algo de texto.\n")).collect::<String>();
        let parts = split_parts(&text, 500);
        assert!(parts.len() > 5);
        assert!(parts.iter().all(|p| p.chars().count() <= 500));
        assert_eq!(parts.concat(), text);
        assert_eq!(split_parts("", 10).len(), 1);
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of(Path::new("a.PDF")), DocKind::Pdf);
        assert_eq!(kind_of(Path::new("a.docx")), DocKind::Docx);
        assert_eq!(kind_of(Path::new("a.jpg")), DocKind::Image);
        assert_eq!(kind_of(Path::new("main.rs")), DocKind::Text);
        assert_eq!(kind_of(Path::new("a.exe")), DocKind::Unsupported);
    }
}
