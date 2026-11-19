//! Voz 100 % local (sin internet, sin créditos):
//! - Escuchar: whisper.cpp (whisper-rs, MIT/Unlicense) dentro de Canela, en la CPU.
//! - Hablar: Piper (MIT) como programa aparte, una vez por frase (no deja
//!   memoria ocupada al terminar).
//!
//! Los modelos NO vienen con el instalador: el usuario elige cuál descargar
//! viendo su tamaño y licencia. Cada archivo se comprueba con SHA-256 antes
//! de usarlo. Las descargas sólo ocurren al pulsar «Descargar».

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Modelo de reconocimiento (whisper).
    Stt,
    /// Voz de Piper (.onnx + .json).
    Voice,
    /// Programa Piper para Windows.
    Engine,
}

#[derive(Debug, Clone, Copy)]
pub struct RemoteFile {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub id: &'static str,
    pub kind: Kind,
    pub label: &'static str,
    pub note: &'static str,
    pub license: &'static str,
    pub files: &'static [RemoteFile],
}

const HF_WHISPER: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

macro_rules! whisper {
    ($name:literal, $sha:literal, $size:literal) => {
        RemoteFile {
            name: $name,
            url: concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", $name),
            sha256: $sha,
            size: $size,
        }
    };
}

macro_rules! piper_voice {
    ($dir:literal, $name:literal, $onnx_sha:literal, $onnx_size:literal, $json_sha:literal, $json_size:literal) => {
        &[
            RemoteFile {
                name: concat!($name, ".onnx"),
                url: concat!(
                    "https://huggingface.co/rhasspy/piper-voices/resolve/main/",
                    $dir,
                    "/",
                    $name,
                    ".onnx"
                ),
                sha256: $onnx_sha,
                size: $onnx_size,
            },
            RemoteFile {
                name: concat!($name, ".onnx.json"),
                url: concat!(
                    "https://huggingface.co/rhasspy/piper-voices/resolve/main/",
                    $dir,
                    "/",
                    $name,
                    ".onnx.json"
                ),
                sha256: $json_sha,
                size: $json_size,
            },
        ]
    };
}

/// Catálogo verificado (tamaños y SHA-256 publicados por Hugging Face y GitHub).
pub const CATALOG: &[Asset] = &[
    Asset {
        id: "stt-small",
        kind: Kind::Stt,
        label: "Reconocimiento recomendado (whisper small)",
        note: "Buen español. ≈ 0,5 GB de RAM mientras está cargado; 1–2 s por frase en tu procesador.",
        license: "MIT",
        files: &[whisper!(
            "ggml-small-q5_1.bin",
            "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
            190_085_487
        )],
    },
    Asset {
        id: "stt-base",
        kind: Kind::Stt,
        label: "Reconocimiento rápido (whisper base)",
        note: "Más ligero (≈ 0,2 GB de RAM) pero se equivoca más en español.",
        license: "MIT",
        files: &[whisper!(
            "ggml-base-q5_1.bin",
            "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
            59_707_625
        )],
    },
    Asset {
        id: "stt-turbo",
        kind: Kind::Stt,
        label: "Máxima precisión (whisper large-v3-turbo)",
        note: "El más preciso. ≈ 1,2 GB de RAM mientras está cargado y más lento (3–5 s por frase).",
        license: "MIT",
        files: &[whisper!(
            "ggml-large-v3-turbo-q5_0.bin",
            "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
            574_041_195
        )],
    },
    Asset {
        id: "stt-tiny",
        kind: Kind::Stt,
        label: "Detector de «Hola, Canela» (whisper tiny)",
        note: "Muy ligero (≈ 0,1 GB). Sólo se usa para la activación por voz.",
        license: "MIT",
        files: &[whisper!(
            "ggml-tiny-q5_1.bin",
            "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7",
            32_152_673
        )],
    },
    Asset {
        id: "voice-davefx",
        kind: Kind::Voice,
        label: "Voz «Dave» (español de España)",
        note: "Voz masculina clara.",
        license: "CC0 (dominio público)",
        files: piper_voice!(
            "es/es_ES/davefx/medium",
            "es_ES-davefx-medium",
            "d2c9224e122321aac3c33343957fff4d0f465226cb74c66b0f520ca3f8c0dd07",
            63_221_984,
            "7d82d069eccca0f87963bd23c33166fe1ef6d24a6b231543931bfe692397f256",
            6_175
        ),
    },
    Asset {
        id: "voice-claude",
        kind: Kind::Voice,
        label: "Voz «Claude» (español de México, alta calidad)",
        note: "Voz femenina natural.",
        license: "Apache-2.0",
        files: piper_voice!(
            "es/es_MX/claude/high",
            "es_MX-claude-high",
            "3ef40a71ea63852cd8ab7e6fa7d2ecdcfa67a0b47c9c48e3f10e02ee02083ea0",
            63_122_309,
            "1afc81f703c0e4cb3b4d7c0dca096b8b54a98806807f0170cf5eb5557723c12d",
            4_963
        ),
    },
    Asset {
        id: "voice-ald",
        kind: Kind::Voice,
        label: "Voz «Ald» (español de México)",
        note: "Voz masculina.",
        license: "Unlicense (dominio público)",
        files: piper_voice!(
            "es/es_MX/ald/medium",
            "es_MX-ald-medium",
            "6ca4b9bc22b3fa19792c212822186fa0f34cb73c1bdced6be83e4038aa946e10",
            63_221_984,
            "94bd0137c7736b2b13ec5c04943e4a0a121b2793ad4bf20c64d625365839edc9",
            6_173
        ),
    },
    Asset {
        id: "piper",
        kind: Kind::Engine,
        label: "Programa Piper para Windows (versión 2023.11.14-2)",
        note: "Necesario para las voces naturales. Incluye espeak-ng (GPL-3.0) para la pronunciación.",
        license: "MIT (Piper) · GPL-3.0 (espeak-ng)",
        files: &[RemoteFile {
            name: "piper_windows_amd64.zip",
            url: "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",
            sha256: "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
            size: 22_477_236,
        }],
    },
];

pub fn asset(id: &str) -> Option<&'static Asset> {
    CATALOG.iter().find(|a| a.id == id)
}

pub fn total_size(a: &Asset) -> u64 {
    a.files.iter().map(|f| f.size).sum()
}

/// Carpeta de modelos de voz: `%APPDATA%\com.canela.ai\voz`.
pub fn voice_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("voz")
}

pub fn piper_exe(dir: &Path) -> PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(p) = std::env::var("CANELA_TEST_PIPER_BIN") {
        return PathBuf::from(p);
    }
    dir.join("piper").join(if cfg!(windows) { "piper.exe" } else { "piper" })
}

pub fn is_installed(dir: &Path, a: &Asset) -> bool {
    match a.kind {
        Kind::Engine => piper_exe(dir).exists(),
        _ => a.files.iter().all(|f| dir.join(f.name).metadata().is_ok_and(|m| m.len() == f.size)),
    }
}

// ------------------------------------------------------------ descargas

static CANCEL: AtomicBool = AtomicBool::new(false);
static DOWNLOADING: Mutex<Option<String>> = Mutex::new(None);

pub fn cancel_download() {
    CANCEL.store(true, Ordering::SeqCst);
}

pub fn downloading() -> Option<String> {
    DOWNLOADING.lock().unwrap().clone()
}

/// Descarga un recurso del catálogo comprobando tamaño y SHA-256.
/// `progress(hecho, total)`.
pub async fn download(http: &reqwest::Client, dir: &Path, a: &Asset, progress: &(dyn Fn(u64, u64) + Send + Sync)) -> Result<(), String> {
    use futures_util::StreamExt;
    {
        let mut d = DOWNLOADING.lock().unwrap();
        if let Some(other) = d.as_ref() {
            return Err(format!("Ya estoy descargando «{other}». Espera a que termine."));
        }
        *d = Some(a.id.to_string());
    }
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            *DOWNLOADING.lock().unwrap() = None;
        }
    }
    let _clear = Clear;
    CANCEL.store(false, Ordering::SeqCst);
    std::fs::create_dir_all(dir).map_err(|e| format!("No pude crear la carpeta de voz: {e}"))?;
    let total = total_size(a);
    let mut done = 0u64;
    for f in a.files {
        let target = dir.join(f.name);
        if target.metadata().is_ok_and(|m| m.len() == f.size) && a.kind != Kind::Engine {
            done += f.size;
            continue;
        }
        let part = dir.join(format!("{}.part", f.name));
        let resp = http
            .get(url_for(f))
            .send()
            .await
            .map_err(|e| format!("No pude descargar {}: {e}", f.name))?;
        if !resp.status().is_success() {
            return Err(format!("El servidor respondió {} al descargar {}.", resp.status(), f.name));
        }
        let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut got = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if CANCEL.load(Ordering::SeqCst) {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err("Descarga cancelada.".into());
            }
            let chunk = chunk.map_err(|e| format!("Se cortó la descarga: {e}"))?;
            got += chunk.len() as u64;
            if got > f.size {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(format!("{} es más grande de lo esperado: descarga rechazada.", f.name));
            }
            hasher.update(&chunk);
            std::io::Write::write_all(&mut file, &chunk).map_err(|e| format!("No pude guardar {}: {e}", f.name))?;
            progress(done + got, total);
        }
        drop(file);
        let hash = hex(&hasher.finalize());
        if got != f.size || hash != f.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!(
                "{} no pasó la comprobación de seguridad (tamaño o SHA-256 distinto). No se usará.",
                f.name
            ));
        }
        std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
        done += f.size;
        if a.kind == Kind::Engine {
            let zip = target.clone();
            let out = dir.to_path_buf();
            tokio::task::spawn_blocking(move || extract_zip(&zip, &out))
                .await
                .map_err(|e| e.to_string())??;
            let _ = std::fs::remove_file(&target);
        }
    }
    progress(total, total);
    Ok(())
}

fn url_for(f: &RemoteFile) -> String {
    #[cfg(test)]
    if let Ok(base) = std::env::var("CANELA_TEST_ASSET_BASE") {
        return format!("{base}/{}", f.name);
    }
    let _ = HF_WHISPER;
    f.url.to_string()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extrae el zip de Piper sin permitir rutas fuera de la carpeta.
pub fn extract_zip(zip_path: &Path, out: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("El zip de Piper está dañado: {e}"))?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let dest = out.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut w = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut w).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Borra un recurso descargado.
pub fn remove(dir: &Path, a: &Asset) -> Result<(), String> {
    if a.kind == Kind::Engine {
        let p = dir.join("piper");
        if p.exists() {
            std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if a.kind == Kind::Stt {
        unload_all();
    }
    for f in a.files {
        let p = dir.join(f.name);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------ whisper

#[cfg(feature = "local-stt")]
struct Loaded {
    id: String,
    ctx: whisper_rs::WhisperContext,
    last_used: Instant,
}

#[cfg(feature = "local-stt")]
static STT: Mutex<Option<Loaded>> = Mutex::new(None);

pub fn stt_available() -> bool {
    cfg!(feature = "local-stt")
}

/// Hilos para whisper: deja núcleos libres para el resto del equipo.
#[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
pub fn stt_threads() -> i32 {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (n.saturating_sub(1)).clamp(2, 8) as i32
}

/// Ventana de audio de whisper ajustada a la duración real (siempre procesa
/// 30 s; para frases cortas basta una ventana menor y es varias veces más
/// rápido). 50 «frames» por segundo, con margen, mínimo ≈ 10 s.
#[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
pub fn audio_ctx_for(samples: usize) -> i32 {
    #[cfg(test)]
    if let Ok(v) = std::env::var("CANELA_TEST_AUDIO_CTX") {
        return v.parse().unwrap_or(0);
    }
    let secs = samples as f32 / 16_000.0;
    if secs >= 25.0 {
        return 0; // ventana completa
    }
    let frames = ((secs + 2.0) * 50.0).ceil() as i32;
    ((frames + 63) / 64 * 64).clamp(512, 1500)
}

/// Transcribe audio mono a 16 kHz con el modelo local.
#[cfg(feature = "local-stt")]
pub fn transcribe(dir: &Path, model_id: &str, samples: &[f32], prompt: Option<&str>) -> Result<String, String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    let a = asset(model_id)
        .filter(|a| a.kind == Kind::Stt)
        .ok_or("Modelo de voz desconocido.")?;
    if !is_installed(dir, a) {
        return Err("Falta descargar el modelo de reconocimiento de voz (Ajustes › Voz).".into());
    }
    let mut guard = STT.lock().unwrap();
    if guard.as_ref().is_none_or(|l| l.id != model_id) {
        *guard = None; // libera el anterior antes de cargar otro
        whisper_rs::install_logging_hooks();
        let mut p = WhisperContextParameters::default();
        p.use_gpu(false);
        let ctx =
            WhisperContext::new_with_params(dir.join(a.files[0].name), p).map_err(|e| format!("No pude cargar el modelo de voz: {e}"))?;
        *guard = Some(Loaded {
            id: model_id.to_string(),
            ctx,
            last_used: Instant::now(),
        });
    }
    let loaded = guard.as_mut().unwrap();
    let mut state = loaded.ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("es"));
    params.set_n_threads(stt_threads());
    params.set_no_context(true);
    params.set_suppress_blank(true);
    params.set_no_timestamps(true);
    params.set_audio_ctx(audio_ctx_for(samples.len()));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    if let Some(p) = prompt {
        params.set_initial_prompt(p);
    }
    state.full(params, samples).map_err(|e| format!("No pude transcribir: {e}"))?;
    let mut text = String::new();
    for i in 0..state.full_n_segments() {
        if let Some(seg) = state.get_segment(i) {
            if let Ok(t) = seg.to_str_lossy() {
                text.push_str(&t);
            }
        }
    }
    loaded.last_used = Instant::now();
    Ok(clean_transcript(&text))
}

#[cfg(not(feature = "local-stt"))]
pub fn transcribe(_dir: &Path, _model_id: &str, _samples: &[f32], _prompt: Option<&str>) -> Result<String, String> {
    Err("Esta compilación no incluye el reconocimiento de voz local.".into())
}

/// Quita marcas que whisper inventa con silencio o ruido.
#[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
pub fn clean_transcript(t: &str) -> String {
    let t = t.trim();
    const NOISE: [&str; 8] = [
        "[música]",
        "[musica]",
        "(música)",
        "[silencio]",
        "[blank_audio]",
        "subtítulos realizados por la comunidad de amara.org",
        "¡suscríbete!",
        "gracias por ver el video.",
    ];
    let lower = t.to_lowercase();
    if NOISE.iter().any(|n| lower == *n) || t.chars().all(|c| !c.is_alphanumeric()) {
        return String::new();
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ¿Qué modelo de reconocimiento está en memoria?
pub fn loaded_stt() -> Option<String> {
    #[cfg(feature = "local-stt")]
    return STT.lock().unwrap().as_ref().map(|l| l.id.clone());
    #[cfg(not(feature = "local-stt"))]
    None
}

/// Descarga el modelo de voz si lleva `idle` sin usarse.
pub fn maybe_unload(idle: Duration) -> bool {
    #[cfg(feature = "local-stt")]
    {
        let mut g = STT.lock().unwrap();
        if g.as_ref().is_some_and(|l| l.last_used.elapsed() >= idle) {
            *g = None;
            return true;
        }
    }
    let _ = idle;
    false
}

/// Libera los modelos de voz cargados. Devuelve `true` si había alguno.
pub fn unload_all() -> bool {
    #[cfg(feature = "local-stt")]
    return STT.lock().unwrap().take().is_some();
    #[cfg(not(feature = "local-stt"))]
    false
}

// ------------------------------------------------------------ Piper

/// Frecuencia de muestreo de una voz (de su .onnx.json).
pub fn voice_rate(dir: &Path, voice: &Asset) -> u32 {
    voice
        .files
        .iter()
        .find(|f| f.name.ends_with(".json"))
        .and_then(|f| std::fs::read_to_string(dir.join(f.name)).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["audio"]["sample_rate"].as_u64())
        .unwrap_or(22_050) as u32
}

/// Sintetiza una frase con Piper: devuelve PCM 16 bits mono. El proceso
/// termina al acabar (no queda memoria ocupada). `stop` lo interrumpe.
pub fn piper_say(dir: &Path, voice: &Asset, text: &str, rate: f32, stop: &AtomicBool) -> Result<Vec<i16>, String> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let exe = piper_exe(dir);
    if !exe.exists() {
        return Err("Falta descargar el programa Piper (Ajustes › Voz).".into());
    }
    if !is_installed(dir, voice) {
        return Err("Falta descargar la voz elegida (Ajustes › Voz).".into());
    }
    let onnx = dir.join(voice.files[0].name);
    // length_scale > 1 = más lento.
    let length_scale = format!("{:.2}", (1.0 / rate.clamp(0.5, 2.0)).clamp(0.5, 2.0));
    let mut cmd = Command::new(&exe);
    cmd.arg("--model")
        .arg(&onnx)
        .args([
            "--output-raw",
            "--length_scale",
            &length_scale,
            "--sentence_silence",
            "0.15",
            "--quiet",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(parent) = exe.parent() {
        cmd.current_dir(parent);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // sin ventana
    }
    let mut child = cmd.spawn().map_err(|e| format!("No pude iniciar Piper: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("Piper sin entrada")?;
        // Una sola línea: Piper lee hasta el salto de línea.
        let line = text.replace(['\n', '\r'], " ");
        stdin.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    let mut out = child.stdout.take().ok_or("Piper sin salida")?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 16384];
    let started = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("interrumpido".into());
        }
        match out.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(e) => {
                let _ = child.kill();
                return Err(e.to_string());
            }
        }
    }
    let _ = child.wait();
    Ok(bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete_and_licensed() {
        for a in CATALOG {
            assert!(!a.files.is_empty(), "{}", a.id);
            assert!(!a.license.is_empty());
            for f in a.files {
                assert_eq!(f.sha256.len(), 64, "{}", f.name);
                assert!(f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(f.url.starts_with("https://huggingface.co/") || f.url.starts_with("https://github.com/rhasspy/piper/"));
                assert!(f.url.ends_with(f.name));
                assert!(f.size > 0);
            }
        }
        assert!(asset("stt-small").is_some());
        assert_eq!(total_size(asset("voice-claude").unwrap()), 63_122_309 + 4_963);
    }

    #[test]
    fn transcripts_are_cleaned() {
        assert_eq!(clean_transcript("  Hola,   Canela.  "), "Hola, Canela.");
        assert_eq!(clean_transcript("[Música]"), "");
        assert_eq!(clean_transcript("..."), "");
        assert_eq!(clean_transcript("Subtítulos realizados por la comunidad de Amara.org"), "");
    }

    #[test]
    fn zip_extraction_is_contained() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("canela-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zp = dir.join("p.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
        let o: zip::write::SimpleFileOptions = Default::default();
        z.start_file("piper/piper.exe", o).unwrap();
        z.write_all(b"exe").unwrap();
        z.start_file("../../fuera.txt", o).unwrap();
        z.write_all(b"malo").unwrap();
        z.finish().unwrap();
        let out = dir.join("voz");
        extract_zip(&zp, &out).unwrap();
        assert!(out.join("piper/piper.exe").exists());
        assert!(!dir.join("fuera.txt").exists() && !std::env::temp_dir().join("fuera.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lee un WAV PCM 16 bits mono y lo pasa a 16 kHz.
    fn read_wav_16k(path: &Path) -> Vec<f32> {
        let b = std::fs::read(path).unwrap();
        let rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
        let pos = b.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<f32> = b[pos..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();
        let mut out = Vec::new();
        crate::voice::audio::Resampler::new(rate, 16_000).push(&pcm, &mut out);
        out
    }

    fn words(t: &str) -> Vec<String> {
        t.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    fn rss_mb() -> f64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmRSS"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
            })
            .unwrap_or(0.0)
            / 1024.0
    }

    /// Prueba REAL sin internet con el modelo whisper descargado.
    /// `CANELA_WHISPER_DIR` = carpeta con ggml-small-q5_1.bin y
    /// `CANELA_PHRASES_DIR` = carpeta con fN.wav + fN.txt (texto esperado).
    #[cfg(feature = "local-stt")]
    #[test]
    fn real_offline_transcription() {
        let (Ok(dir), Ok(phrases)) = (std::env::var("CANELA_WHISPER_DIR"), std::env::var("CANELA_PHRASES_DIR")) else {
            eprintln!("omitida: define CANELA_WHISPER_DIR y CANELA_PHRASES_DIR");
            return;
        };
        let dir = PathBuf::from(dir);
        assert!(
            is_installed(&dir, asset("stt-small").unwrap()),
            "el modelo debe pasar la comprobación de tamaño"
        );
        let before = rss_mb();
        let mut total_recall = 0.0;
        let mut n = 0;
        for i in 1..=20 {
            let wav = PathBuf::from(&phrases).join(format!("f{i}.wav"));
            if !wav.exists() {
                break;
            }
            let expected = std::fs::read_to_string(wav.with_extension("txt")).unwrap();
            let audio = read_wav_16k(&wav);
            let t0 = Instant::now();
            let got = transcribe(&dir, "stt-small", &audio, Some("Hola, Canela.")).unwrap();
            let secs = t0.elapsed().as_secs_f32();
            let exp = words(&expected);
            let gw = words(&got);
            let hit = exp.iter().filter(|w| gw.contains(w)).count();
            let recall = hit as f64 / exp.len() as f64;
            total_recall += recall;
            n += 1;
            eprintln!(
                "[{:.1} s audio → {secs:.2} s] esperado: «{}» · entendido: «{got}» · palabras acertadas {:.0} %",
                audio.len() as f32 / 16_000.0,
                expected.trim(),
                recall * 100.0
            );
        }
        let loaded = rss_mb();
        assert_eq!(loaded_stt().as_deref(), Some("stt-small"));
        assert!(unload_all());
        let after = rss_mb();
        eprintln!("RAM del proceso: antes {before:.0} MB · con el modelo cargado {loaded:.0} MB · tras liberarlo {after:.0} MB");
        assert!(n > 0);
        assert!(
            total_recall / n as f64 >= 0.6,
            "acierto medio {:.0} %",
            total_recall / n as f64 * 100.0
        );
        assert!(loaded_stt().is_none());
    }
}
