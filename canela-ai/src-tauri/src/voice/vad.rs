//! Detector de actividad de voz (VAD) por energía, con piso de ruido adaptativo.
//!
//! Decide cuándo empezaste a hablar y cuándo terminaste (silencio sostenido),
//! sin enviar nada a internet. Trabaja con audio mono a cualquier frecuencia.

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    pub frame_ms: u32,
    /// Factor sobre el ruido de fondo para considerar que hay voz.
    pub start_factor: f32,
    /// Nivel mínimo absoluto (RMS) para considerar voz.
    pub min_level: f32,
    /// Silencio necesario tras hablar para dar por terminada la frase.
    pub silence_end_ms: u32,
    /// Duración máxima de una grabación.
    pub max_ms: u32,
    /// Si no hablas en este tiempo, se cancela.
    pub no_speech_ms: u32,
    /// Tiempo inicial para medir el ruido de fondo.
    pub calib_ms: u32,
    /// Fotogramas seguidos sobre el umbral para empezar.
    pub start_frames: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 30,
            start_factor: 3.0,
            min_level: 0.012,
            silence_end_ms: 900,
            max_ms: 45_000,
            no_speech_ms: 8_000,
            calib_ms: 240,
            start_frames: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VadEvent {
    SpeechStarted,
    /// Terminaste de hablar.
    Ended,
    /// No se detectó voz a tiempo.
    NoSpeech,
    /// Se alcanzó la duración máxima.
    TooLong,
}

pub struct Vad {
    cfg: VadConfig,
    frame_len: usize,
    buf: Vec<f32>,
    noise: f32,
    frames: u32,
    calib_sum: f32,
    above: u32,
    speaking: bool,
    silence: u32,
    done: bool,
    pub level: f32,
}

impl Vad {
    pub fn new(sample_rate: u32, cfg: VadConfig) -> Self {
        let frame_len = (sample_rate as usize * cfg.frame_ms as usize / 1000).max(1);
        Self {
            cfg,
            frame_len,
            buf: Vec::with_capacity(frame_len),
            noise: 0.0,
            frames: 0,
            calib_sum: 0.0,
            above: 0,
            speaking: false,
            silence: 0,
            done: false,
            level: 0.0,
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    fn threshold(&self) -> f32 {
        (self.noise * self.cfg.start_factor).max(self.cfg.min_level)
    }

    fn ms(&self, frames: u32) -> u32 {
        frames * self.cfg.frame_ms
    }

    fn frame(&mut self, rms: f32) -> Option<VadEvent> {
        self.frames += 1;
        self.level = rms;
        let calib_frames = (self.cfg.calib_ms / self.cfg.frame_ms).max(1);
        if self.frames <= calib_frames {
            self.calib_sum += rms;
            self.noise = self.calib_sum / self.frames as f32;
            return None;
        }
        let loud = rms > self.threshold();
        if !self.speaking {
            if loud {
                self.above += 1;
                if self.above >= self.cfg.start_frames {
                    self.speaking = true;
                    self.silence = 0;
                    return Some(VadEvent::SpeechStarted);
                }
            } else {
                self.above = 0;
                // El ruido de fondo se adapta lentamente mientras no hablas.
                self.noise = self.noise * 0.95 + rms * 0.05;
            }
            if self.ms(self.frames) >= self.cfg.no_speech_ms {
                return Some(VadEvent::NoSpeech);
            }
        } else {
            if loud {
                self.silence = 0;
            } else {
                self.silence += 1;
                if self.ms(self.silence) >= self.cfg.silence_end_ms {
                    return Some(VadEvent::Ended);
                }
            }
        }
        if self.ms(self.frames) >= self.cfg.max_ms {
            return Some(VadEvent::TooLong);
        }
        None
    }

    /// Procesa muestras y devuelve el evento final (si lo hay).
    pub fn push(&mut self, samples: &[f32]) -> Vec<VadEvent> {
        let mut out = Vec::new();
        for &s in samples {
            if self.done {
                break;
            }
            self.buf.push(s);
            if self.buf.len() == self.frame_len {
                let rms =
                    (self.buf.iter().map(|x| x * x).sum::<f32>() / self.frame_len as f32).sqrt();
                self.buf.clear();
                if let Some(ev) = self.frame(rms) {
                    if ev != VadEvent::SpeechStarted {
                        self.done = true;
                    }
                    out.push(ev);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn noise(ms: u32, amp: f32, seed: &mut u32) -> Vec<f32> {
        (0..SR * ms / 1000)
            .map(|_| {
                *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                ((*seed >> 16) as f32 / 32768.0 - 1.0) * amp
            })
            .collect()
    }

    fn tone(ms: u32, amp: f32) -> Vec<f32> {
        (0..SR * ms / 1000)
            .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / SR as f32).sin() * amp)
            .collect()
    }

    #[test]
    fn detects_speech_then_end_of_phrase() {
        let mut seed = 7;
        let mut v = Vad::new(SR, VadConfig::default());
        let mut evs = v.push(&noise(500, 0.005, &mut seed));
        assert!(evs.is_empty());
        evs.extend(v.push(&tone(1200, 0.3)));
        assert_eq!(evs, vec![VadEvent::SpeechStarted]);
        // Una pausa corta NO termina la frase.
        evs.extend(v.push(&noise(400, 0.005, &mut seed)));
        evs.extend(v.push(&tone(500, 0.3)));
        assert_eq!(evs.len(), 1);
        evs.extend(v.push(&noise(1200, 0.005, &mut seed)));
        assert_eq!(evs.last(), Some(&VadEvent::Ended));
    }

    #[test]
    fn gives_up_without_speech() {
        let mut seed = 3;
        let mut v = Vad::new(SR, VadConfig::default());
        let evs = v.push(&noise(9000, 0.004, &mut seed));
        assert_eq!(evs, vec![VadEvent::NoSpeech]);
    }

    #[test]
    fn adapts_to_noisy_room() {
        // Ruido de fondo alto (ventilador): no debe confundirlo con voz.
        let mut seed = 11;
        let mut v = Vad::new(
            SR,
            VadConfig {
                no_speech_ms: 20_000,
                ..VadConfig::default()
            },
        );
        assert!(v.push(&noise(3000, 0.05, &mut seed)).is_empty());
        assert_eq!(v.push(&tone(600, 0.5)), vec![VadEvent::SpeechStarted]);
    }

    #[test]
    fn stops_at_max_duration() {
        let mut v = Vad::new(
            SR,
            VadConfig {
                max_ms: 2000,
                ..VadConfig::default()
            },
        );
        let mut seed = 5;
        v.push(&noise(300, 0.004, &mut seed));
        let evs = v.push(&tone(3000, 0.3));
        assert_eq!(evs, vec![VadEvent::SpeechStarted, VadEvent::TooLong]);
    }
}
