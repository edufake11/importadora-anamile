//! «Hola, Canela»: activación por voz 100 % local.
//!
//! - Desactivada por defecto; sólo se enciende con permiso expreso.
//! - Mientras está activa hay un indicador visible (en Canela y en el panel)
//!   y se puede apagar con un clic.
//! - Un detector de voz muy ligero busca ráfagas cortas de habla; sólo esas
//!   se pasan a un modelo whisper pequeño (tiny, en este equipo). El audio se
//!   analiza y se descarta: nunca se guarda ni se envía.
//! - Se pausa mientras Canela graba tu orden o está hablando.

use super::audio::{self, TARGET_RATE};
use super::local;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ------------------------------------------------------------ segmentación

const FRAME: usize = (TARGET_RATE as usize) * 30 / 1000; // 30 ms
const PREROLL_FRAMES: usize = 10; // 300 ms antes de detectar voz
const START_FRAMES: u32 = 3;
const END_SILENCE_FRAMES: u32 = 15; // 450 ms
const MIN_SPEECH_FRAMES: u32 = 10; // 300 ms
const MAX_FRAMES: usize = 200; // 6 s

/// Corta el audio continuo en ráfagas de voz (0,3 – 6 s).
pub struct Segmenter {
    pending: Vec<f32>,
    noise: f32,
    frames: u32,
    preroll: VecDeque<Vec<f32>>,
    current: Option<Vec<Vec<f32>>>,
    above: u32,
    silence: u32,
    speech: u32,
    /// Tras una ráfaga demasiado larga, esperar a una pausa antes de volver a escuchar.
    wait_for_pause: bool,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            noise: 0.0,
            frames: 0,
            preroll: VecDeque::new(),
            current: None,
            above: 0,
            silence: 0,
            speech: 0,
            wait_for_pause: false,
        }
    }
}

impl Segmenter {
    fn threshold(&self) -> f32 {
        (self.noise * 3.0).max(0.012)
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.pending.len() >= FRAME {
            let frame: Vec<f32> = self.pending.drain(..FRAME).collect();
            if let Some(seg) = self.frame(frame) {
                out.push(seg);
            }
        }
        out
    }

    fn frame(&mut self, frame: Vec<f32>) -> Option<Vec<f32>> {
        let rms = (frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
        self.frames += 1;
        if self.frames <= 8 {
            self.noise = if self.frames == 1 { rms } else { self.noise * 0.8 + rms * 0.2 };
            return None;
        }
        let loud = rms > self.threshold();
        if self.wait_for_pause {
            if loud {
                self.silence = 0;
            } else {
                self.silence += 1;
                if self.silence >= END_SILENCE_FRAMES {
                    self.wait_for_pause = false;
                    self.silence = 0;
                }
            }
            return None;
        }
        match self.current.as_mut() {
            None => {
                self.preroll.push_back(frame);
                if self.preroll.len() > PREROLL_FRAMES {
                    self.preroll.pop_front();
                }
                if loud {
                    self.above += 1;
                    if self.above >= START_FRAMES {
                        self.current = Some(self.preroll.drain(..).collect());
                        self.speech = self.above;
                        self.silence = 0;
                        self.above = 0;
                    }
                } else {
                    self.above = 0;
                    self.noise = self.noise * 0.97 + rms * 0.03;
                }
                None
            }
            Some(cur) => {
                cur.push(frame);
                if loud {
                    self.silence = 0;
                    self.speech += 1;
                } else {
                    self.silence += 1;
                }
                if cur.len() > MAX_FRAMES {
                    // Demasiado largo para ser la frase de activación.
                    self.current = None;
                    self.wait_for_pause = true;
                    self.silence = 0;
                    return None;
                }
                if self.silence >= END_SILENCE_FRAMES {
                    let seg = self.current.take().unwrap();
                    if self.speech >= MIN_SPEECH_FRAMES {
                        return Some(seg.concat());
                    }
                }
                None
            }
        }
    }
}

// ------------------------------------------------------------ frase

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

fn near(a: &str, b: &str) -> bool {
    // Distancia de edición ≤ 1 («canella», «canelas», «kanela»).
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()] <= 1
}

#[derive(Debug, PartialEq)]
pub enum Wake {
    /// No es la frase de activación.
    No,
    /// «Hola, Canela» sola: empezar a escuchar la orden.
    Listen,
    /// «Hola, Canela, abre la calculadora»: la orden ya viene incluida.
    Command(String),
}

/// ¿Es «(hola|oye|hey) Canela…» al principio de lo que se dijo?
pub fn detect(transcript: &str) -> Wake {
    let words: Vec<&str> = transcript.split_whitespace().collect();
    let folded: Vec<String> = words.iter().map(|w| fold(w).trim().to_string()).collect();
    let Some(idx) = folded.iter().position(|w| near(w, "canela")) else {
        return Wake::No;
    };
    let lead_ok = folded[..idx].iter().all(|w| {
        w.is_empty()
            || [
                "hola", "ola", "oye", "hey", "ey", "ok", "okay", "buenas", "buenos", "dias", "ah", "eh",
            ]
            .contains(&w.as_str())
    });
    if idx > 3 || !lead_ok {
        return Wake::No;
    }
    let rest: Vec<&str> = words[idx + 1..].to_vec();
    let rest_text = rest
        .join(" ")
        .trim_start_matches([',', '.', '!', '¡', '?', '¿', ' '])
        .trim()
        .to_string();
    if rest_text.split_whitespace().count() >= 2 {
        Wake::Command(rest_text)
    } else {
        Wake::Listen
    }
}

// ------------------------------------------------------------ servicio

pub struct WakeRuntime {
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

pub static RUNTIME: WakeRuntime = WakeRuntime { stop: Mutex::new(None) };

pub fn is_running() -> bool {
    RUNTIME.stop.lock().unwrap().is_some()
}

/// Arranca la escucha. `paused()` indica si hay que ignorar el audio (Canela
/// graba o habla). `on_wake` recibe lo detectado.
pub fn start(
    dir: std::path::PathBuf,
    device: Option<String>,
    // Modelo principal (más preciso) para la orden que venga tras la frase.
    main_model: Option<String>,
    paused: Arc<dyn Fn() -> bool + Send + Sync>,
    on_wake: Arc<dyn Fn(Wake) + Send + Sync>,
    on_error: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<(), String> {
    if !local::stt_available() {
        return Err("Esta compilación no incluye el reconocimiento de voz local.".into());
    }
    if !local::is_installed(&dir, local::asset("stt-tiny").unwrap()) {
        return Err("Falta descargar el detector de «Hola, Canela» (32 MB) en Ajustes › Voz.".into());
    }
    let mut slot = RUNTIME.stop.lock().unwrap();
    if slot.is_some() {
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    *slot = Some(stop.clone());
    drop(slot);
    std::thread::spawn(move || {
        let err2 = on_error.clone();
        let (stream, rx) = match audio::open_input(device.as_deref(), Arc::new(move |e| err2(e))) {
            Ok(x) => x,
            Err(e) => {
                *RUNTIME.stop.lock().unwrap() = None;
                return on_error(e);
            }
        };
        let mut seg = Segmenter::default();
        let mut cooldown_until = std::time::Instant::now();
        while !stop.load(Ordering::SeqCst) {
            let chunk = match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(c) => c,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            };
            if paused() || std::time::Instant::now() < cooldown_until {
                seg = Segmenter::default(); // descarta lo que había
                continue;
            }
            for s in seg.push(&chunk) {
                let text = local::transcribe_wake(&dir, &s).unwrap_or_default();
                let mut w = detect(&text);
                if let (Wake::Command(_), Some(m)) = (&w, main_model.as_deref()) {
                    // La orden se vuelve a transcribir con el modelo principal.
                    if let Ok(t2) = local::transcribe(&dir, m, &s, Some("Hola, Canela.")) {
                        if let Wake::Command(c2) = detect(&t2) {
                            w = Wake::Command(c2);
                        }
                    }
                }
                drop(s); // el audio no se guarda
                if w != Wake::No {
                    cooldown_until = std::time::Instant::now() + Duration::from_secs(3);
                    on_wake(w);
                }
            }
        }
        drop(stream); // cierra el micrófono
        local::unload_wake();
        *RUNTIME.stop.lock().unwrap() = None;
    });
    Ok(())
}

pub fn stop() {
    if let Some(s) = RUNTIME.stop.lock().unwrap().take() {
        s.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize, amp: f32) -> Vec<f32> {
        (0..TARGET_RATE as usize * ms / 1000)
            .map(|i| (i as f32 * 0.07).sin() * amp)
            .collect()
    }

    #[test]
    fn segments_short_bursts_of_speech_only() {
        let mut s = Segmenter::default();
        let mut segs = s.push(&tone(1000, 0.002)); // silencio de fondo
        segs.extend(s.push(&tone(900, 0.3))); // «Hola, Canela»
        segs.extend(s.push(&tone(600, 0.002)));
        assert_eq!(segs.len(), 1);
        let secs = segs[0].len() as f32 / TARGET_RATE as f32;
        assert!(secs > 1.0 && secs < 2.0, "{secs}");
        // Un golpe corto (clic) no cuenta.
        let mut segs = s.push(&tone(60, 0.5));
        segs.extend(s.push(&tone(800, 0.002)));
        assert!(segs.is_empty());
        // Hablar 8 s seguidos (una conversación) no se analiza.
        let mut segs = s.push(&tone(8000, 0.3));
        segs.extend(s.push(&tone(800, 0.002)));
        assert!(segs.is_empty());
    }

    #[test]
    fn wake_phrase_detection() {
        assert_eq!(detect("Hola, Canela."), Wake::Listen);
        assert_eq!(detect("¡Oye Canela!"), Wake::Listen);
        assert_eq!(detect("Canela"), Wake::Listen);
        assert_eq!(detect("Hola Canella"), Wake::Listen, "pequeños errores de transcripción");
        assert_eq!(
            detect("Hola, Canela, abre la calculadora."),
            Wake::Command("abre la calculadora.".into())
        );
        assert_eq!(
            detect("Necesito comprar canela para el pastel"),
            Wake::No,
            "mencionarla en una frase no la activa"
        );
        assert_eq!(detect("Hola, ¿cómo estás?"), Wake::No);
        assert_eq!(detect(""), Wake::No);
        assert_eq!(detect("Hola Daniela"), Wake::No);
    }

    /// Prueba REAL: audio continuo (silencio + frase + silencio) → segmentador
    /// → whisper tiny → detección. `CANELA_WHISPER_DIR` con ggml-tiny-q5_1.bin y
    /// `CANELA_WAKE_DIR` con wN.wav / wN.txt.
    #[cfg(feature = "local-stt")]
    #[test]
    fn real_wake_word_end_to_end() {
        let (Ok(dir), Ok(phr)) = (std::env::var("CANELA_WHISPER_DIR"), std::env::var("CANELA_WAKE_DIR")) else {
            eprintln!("omitida: define CANELA_WHISPER_DIR y CANELA_WAKE_DIR");
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let read = |p: &std::path::Path| {
            let b = std::fs::read(p).unwrap();
            let rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
            let pos = b.windows(4).position(|w| w == b"data").unwrap() + 8;
            let pcm: Vec<f32> = b[pos..]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                .collect();
            let mut out = Vec::new();
            audio::Resampler::new(rate, TARGET_RATE).push(&pcm, &mut out);
            out
        };
        let mut ok = 0;
        let mut n = 0;
        for i in 1..=10 {
            let wav = std::path::PathBuf::from(&phr).join(format!("w{i}.wav"));
            if !wav.exists() {
                break;
            }
            let expected_text = std::fs::read_to_string(wav.with_extension("txt")).unwrap();
            let should_wake = detect(&expected_text) != Wake::No;
            // Flujo continuo como el del micrófono, en trozos de 20 ms.
            let mut stream = tone(1000, 0.001);
            stream.extend(read(&wav));
            stream.extend(tone(1200, 0.001));
            let mut seg = Segmenter::default();
            let mut result = Wake::No;
            let mut heard = String::new();
            let t0 = std::time::Instant::now();
            for chunk in stream.chunks(320) {
                for s in seg.push(chunk) {
                    let text = local::transcribe_wake(&dir, &s).unwrap();
                    heard.push_str(&text);
                    let w = detect(&text);
                    if w != Wake::No {
                        result = w;
                    }
                }
            }
            let woke = result != Wake::No;
            n += 1;
            if woke == should_wake {
                ok += 1;
            }
            eprintln!(
                "«{}» → oído «{heard}» → {:?} ({}) en {:.2} s",
                expected_text.trim(),
                result,
                if woke == should_wake { "correcto" } else { "INCORRECTO" },
                t0.elapsed().as_secs_f32()
            );
        }
        local::unload_wake();
        assert!(n >= 4);
        assert!(ok * 100 / n >= 80, "{ok}/{n}");
    }
}
