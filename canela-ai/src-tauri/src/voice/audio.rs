//! Audio nativo (cpal): listar dispositivos, grabar del micrófono y reproducir.
//!
//! - La grabación corre en su propio hilo; el audio se convierte a mono 16 kHz,
//!   pasa por el detector de voz y se queda SÓLO en memoria.
//! - La reproducción (voz de OpenAI) usa el altavoz elegido y se puede cortar.

use super::vad::{Vad, VadConfig, VadEvent};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|x| x.name().to_string())
        .unwrap_or_else(|_| "Dispositivo".into())
}

pub fn list_devices() -> Devices {
    let host = cpal::default_host();
    let inputs = host
        .input_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    let outputs = host
        .output_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    Devices { inputs, outputs }
}

fn find_input(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        if let Ok(mut it) = host.input_devices() {
            if let Some(d) = it.find(|d| device_name(d) == n) {
                return Some(d);
            }
        }
    }
    host.default_input_device()
}

fn find_output(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        if let Ok(mut it) = host.output_devices() {
            if let Some(d) = it.find(|d| device_name(d) == n) {
                return Some(d);
            }
        }
    }
    host.default_output_device()
}

// ------------------------------------------------------------ remuestreo

/// Remuestreo lineal en flujo (suficiente para voz).
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
    started: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            step: from as f64 / to as f64,
            pos: 0.0,
            prev: 0.0,
            started: false,
        }
    }

    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &x in input {
            if !self.started {
                self.prev = x;
                self.started = true;
                continue;
            }
            // Interpola entre prev (t=0) y x (t=1).
            while self.pos <= 1.0 {
                out.push(self.prev + (x - self.prev) * self.pos as f32);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = x;
        }
    }
}

// --------------------------------------------------------------- grabación

pub const CTRL_RUN: u8 = 0;
pub const CTRL_FINISH: u8 = 1;
pub const CTRL_CANCEL: u8 = 2;

#[derive(Debug, Clone)]
pub enum RecEvent {
    Level(f32),
    SpeechStarted,
    /// Audio final (mono 16 kHz).
    Finished(Vec<f32>),
    NoSpeech,
    Cancelled,
    Error(String),
}

/// Bucle de procesamiento independiente del hardware (se prueba sin micrófono).
pub fn process_audio(rx: mpsc::Receiver<Vec<f32>>, ctrl: Arc<AtomicU8>, cfg: VadConfig, emit: &dyn Fn(RecEvent)) {
    let mut vad = Vad::new(TARGET_RATE, cfg);
    let mut all: Vec<f32> = Vec::new();
    let max_samples = (TARGET_RATE as usize) * (cfg.max_ms as usize / 1000 + 1);
    let mut last_level = Instant::now() - Duration::from_secs(1);
    loop {
        match ctrl.load(Ordering::SeqCst) {
            CTRL_CANCEL => return emit(RecEvent::Cancelled),
            CTRL_FINISH => {
                return if vad.is_speaking() || all.len() > TARGET_RATE as usize / 2 {
                    emit(RecEvent::Finished(all))
                } else {
                    emit(RecEvent::NoSpeech)
                }
            }
            _ => {}
        }
        let chunk = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return if vad.is_speaking() {
                    emit(RecEvent::Finished(all))
                } else {
                    emit(RecEvent::Error("El micrófono dejó de enviar audio.".into()))
                }
            }
        };
        if all.len() < max_samples {
            all.extend_from_slice(&chunk);
        }
        for ev in vad.push(&chunk) {
            match ev {
                VadEvent::SpeechStarted => emit(RecEvent::SpeechStarted),
                VadEvent::Ended | VadEvent::TooLong => return emit(RecEvent::Finished(all)),
                VadEvent::NoSpeech => return emit(RecEvent::NoSpeech),
            }
        }
        if last_level.elapsed() >= Duration::from_millis(100) {
            last_level = Instant::now();
            emit(RecEvent::Level(vad.level));
        }
    }
}

/// Graba del micrófono hasta que termines de hablar, pulses detener o canceles.
/// Bloquea el hilo que la llama: úsala desde `std::thread::spawn`.
pub fn record(device: Option<String>, ctrl: Arc<AtomicU8>, emit: Arc<dyn Fn(RecEvent) + Send + Sync>) {
    let Some(dev) = find_input(device.as_deref()) else {
        return emit(RecEvent::Error(
            "No encontré ningún micrófono. Conecta uno o elige otro en Ajustes › Voz.".into(),
        ));
    };
    let config = match dev.default_input_config() {
        Ok(c) => c,
        Err(e) => return emit(RecEvent::Error(format!("No pude usar el micrófono: {e}"))),
    };
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let err_emit = emit.clone();
    let on_err = move |e: cpal::StreamError| err_emit(RecEvent::Error(format!("Error del micrófono: {e}")));

    fn mono<T: Copy>(data: &[T], channels: usize, conv: impl Fn(T) -> f32) -> Vec<f32> {
        data.chunks(channels.max(1))
            .map(|f| f.iter().map(|s| conv(*s)).sum::<f32>() / f.len() as f32)
            .collect()
    }

    let mut rs = Resampler::new(rate, TARGET_RATE);
    let mut send = move |m: Vec<f32>| {
        let mut out = Vec::with_capacity(m.len() / 2);
        rs.push(&m, &mut out);
        let _ = tx.send(out);
    };
    let cfg = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => dev.build_input_stream(&cfg, move |d: &[f32], _: &_| send(mono(d, channels, |s| s)), on_err, None),
        cpal::SampleFormat::I16 => dev.build_input_stream(
            &cfg,
            move |d: &[i16], _: &_| send(mono(d, channels, |s| s as f32 / 32768.0)),
            on_err,
            None,
        ),
        cpal::SampleFormat::U16 => dev.build_input_stream(
            &cfg,
            move |d: &[u16], _: &_| send(mono(d, channels, |s| (s as f32 - 32768.0) / 32768.0)),
            on_err,
            None,
        ),
        cpal::SampleFormat::I32 => dev.build_input_stream(
            &cfg,
            move |d: &[i32], _: &_| send(mono(d, channels, |s| s as f32 / 2_147_483_648.0)),
            on_err,
            None,
        ),
        other => return emit(RecEvent::Error(format!("Formato de micrófono no compatible ({other:?})."))),
    };
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            return emit(RecEvent::Error(format!(
                "No pude abrir el micrófono: {e}. Revisa Configuración de Windows › Privacidad › Micrófono."
            )))
        }
    };
    if let Err(e) = stream.play() {
        return emit(RecEvent::Error(format!("No pude iniciar el micrófono: {e}")));
    }
    process_audio(rx, ctrl, VadConfig::default(), &*emit);
    drop(stream); // cierra el micrófono en cuanto termina
}

// ------------------------------------------------------------- reproducción

/// Reproduce PCM 16 bits mono a `rate` Hz. Devuelve al terminar o si `stop` se activa.
pub fn play_pcm(device: Option<&str>, pcm: &[i16], rate: u32, volume: f32, stop: Arc<AtomicBool>) -> Result<(), String> {
    let dev = find_output(device).ok_or("No encontré ningún altavoz.")?;
    let config = dev.default_output_config().map_err(|e| format!("No pude usar el altavoz: {e}"))?;
    let channels = config.channels() as usize;
    let out_rate = config.sample_rate();
    let mut samples = Vec::with_capacity(pcm.len() * 2);
    let input: Vec<f32> = pcm.iter().map(|s| *s as f32 / 32768.0 * volume.clamp(0.0, 1.0)).collect();
    Resampler::new(rate, out_rate).push(&input, &mut samples);
    let total = samples.len();
    let samples = Arc::new(samples);
    let pos = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cfg = config.config();
    macro_rules! build {
        ($t:ty, $conv:expr) => {{
            let (s, p) = (samples.clone(), pos.clone());
            dev.build_output_stream(
                &cfg,
                move |out: &mut [$t], _: &_| {
                    for frame in out.chunks_mut(channels.max(1)) {
                        let i = p.fetch_add(1, Ordering::Relaxed);
                        let v = s.get(i).copied().unwrap_or(0.0);
                        for x in frame.iter_mut() {
                            *x = $conv(v);
                        }
                    }
                },
                |e| eprintln!("[canela] altavoz: {e}"),
                None,
            )
        }};
    }
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build!(f32, |v: f32| v),
        cpal::SampleFormat::I16 => build!(i16, |v: f32| (v * 32767.0) as i16),
        cpal::SampleFormat::U16 => build!(u16, |v: f32| ((v + 1.0) * 32767.5) as u16),
        other => return Err(format!("Formato de altavoz no compatible ({other:?}).")),
    }
    .map_err(|e| format!("No pude abrir el altavoz: {e}"))?;
    stream.play().map_err(|e| format!("No pude reproducir: {e}"))?;
    while pos.load(Ordering::Relaxed) < total + out_rate as usize / 10 {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_changes_rate() {
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 / 48.0).sin()).collect();
        let mut out = Vec::new();
        Resampler::new(48_000, 16_000).push(&input, &mut out);
        assert!((out.len() as i64 - 16_000).abs() < 5, "{}", out.len());
        let mut up = Vec::new();
        Resampler::new(24_000, 48_000).push(&input[..24_000], &mut up);
        assert!((up.len() as i64 - 48_000).abs() < 5);
    }

    #[test]
    fn processing_loop_finishes_on_silence() {
        let (tx, rx) = mpsc::channel();
        let ctrl = Arc::new(AtomicU8::new(CTRL_RUN));
        // 0,3 s silencio, 1 s "voz", 1,2 s silencio, en trozos de 20 ms.
        let mut audio = vec![0.001f32; 4800];
        audio.extend((0..16_000).map(|i| (i as f32 * 0.09).sin() * 0.3));
        audio.extend(vec![0.001f32; 19_200]);
        for c in audio.chunks(320) {
            tx.send(c.to_vec()).unwrap();
        }
        let events = std::sync::Mutex::new(Vec::new());
        process_audio(rx, ctrl, VadConfig::default(), &|e| events.lock().unwrap().push(e));
        let evs = events.into_inner().unwrap();
        assert!(evs.iter().any(|e| matches!(e, RecEvent::SpeechStarted)));
        match evs.last().unwrap() {
            RecEvent::Finished(s) => assert!(s.len() > 16_000),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn processing_loop_can_be_cancelled_or_finished_manually() {
        let (_tx, rx) = mpsc::channel::<Vec<f32>>();
        let ctrl = Arc::new(AtomicU8::new(CTRL_CANCEL));
        let got = std::sync::Mutex::new(None);
        process_audio(rx, ctrl, VadConfig::default(), &|e| *got.lock().unwrap() = Some(e));
        assert!(matches!(got.into_inner().unwrap(), Some(RecEvent::Cancelled)));

        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        tx.send(vec![0.2; 16_000]).unwrap();
        let ctrl = Arc::new(AtomicU8::new(CTRL_RUN));
        let c2 = ctrl.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c2.store(CTRL_FINISH, Ordering::SeqCst);
        });
        let got = std::sync::Mutex::new(Vec::new());
        process_audio(rx, ctrl, VadConfig::default(), &|e| got.lock().unwrap().push(e));
        assert!(matches!(got.into_inner().unwrap().last(), Some(RecEvent::Finished(_))));
    }
}
