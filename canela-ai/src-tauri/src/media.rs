//! Controles multimedia de Windows: reproducir/pausar, siguiente, anterior,
//! detener, volumen y silencio.
//!
//! Sólo se pulsan las teclas multimedia estándar de una lista fija: la IA
//! elige una acción, nunca una tecla ni un comando. Funcionan con cualquier
//! reproductor que respete esas teclas (Spotify, YouTube en el navegador,
//! Reproductor multimedia…).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    PlayPause,
    Next,
    Previous,
    Stop,
    VolumeUp,
    VolumeDown,
    Mute,
}

/// Cada pulsación de volumen cambia unos 2 puntos en Windows.
pub const MAX_VOLUME_STEPS: u32 = 25;

impl MediaAction {
    pub const NAMES: [&'static str; 7] = [
        "reproducir_pausar",
        "siguiente",
        "anterior",
        "detener",
        "subir_volumen",
        "bajar_volumen",
        "silenciar",
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_lowercase().as_str() {
            "reproducir_pausar" | "reproducir" | "pausar" | "play" | "pausa" => Self::PlayPause,
            "siguiente" => Self::Next,
            "anterior" => Self::Previous,
            "detener" => Self::Stop,
            "subir_volumen" => Self::VolumeUp,
            "bajar_volumen" => Self::VolumeDown,
            "silenciar" | "activar_sonido" => Self::Mute,
            _ => return None,
        })
    }

    /// Código de tecla virtual de Windows.
    pub fn vk(self) -> u16 {
        match self {
            Self::PlayPause => 0xB3,
            Self::Next => 0xB0,
            Self::Previous => 0xB1,
            Self::Stop => 0xB2,
            Self::VolumeUp => 0xAF,
            Self::VolumeDown => 0xAE,
            Self::Mute => 0xAD,
        }
    }

    pub fn label(self, steps: u32) -> String {
        match self {
            Self::PlayPause => "Reproducir / pausar".into(),
            Self::Next => "Siguiente canción".into(),
            Self::Previous => "Canción anterior".into(),
            Self::Stop => "Detener la reproducción".into(),
            Self::VolumeUp => format!("Subir el volumen (~{} %)", steps * 2),
            Self::VolumeDown => format!("Bajar el volumen (~{} %)", steps * 2),
            Self::Mute => "Silenciar / activar el sonido".into(),
        }
    }

    fn is_volume(self) -> bool {
        matches!(self, Self::VolumeUp | Self::VolumeDown)
    }
}

/// Pasos efectivos: el volumen admite 1–25 pulsaciones; el resto, una.
pub fn clamp_steps(action: MediaAction, steps: Option<i64>) -> u32 {
    if action.is_volume() {
        steps.unwrap_or(5).clamp(1, MAX_VOLUME_STEPS as i64) as u32
    } else {
        1
    }
}

/// Secuencia de teclas a pulsar.
pub fn key_sequence(action: MediaAction, steps: u32) -> Vec<u16> {
    vec![action.vk(); steps as usize]
}

#[cfg(windows)]
pub fn press(keys: &[u16]) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    };
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: KEYEVENTF_EXTENDEDKEY | if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    for &vk in keys {
        let inputs = [key(vk, false), key(vk, true)];
        // SAFETY: arreglo válido de 2 INPUT con el tamaño correcto.
        let sent = unsafe { SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        if sent != 2 {
            return Err("Windows no aceptó la tecla multimedia (¿hay una ventana de administrador en primer plano?).".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn press(_keys: &[u16]) -> Result<(), String> {
    Err("Los controles multimedia sólo están disponibles en Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_actions_are_accepted() {
        for n in MediaAction::NAMES {
            assert!(MediaAction::parse(n).is_some(), "{n}");
        }
        assert_eq!(MediaAction::parse("Pausar"), Some(MediaAction::PlayPause));
        assert!(MediaAction::parse("alt+f4").is_none());
        assert!(MediaAction::parse("cmd /c del").is_none());
    }

    #[test]
    fn volume_steps_are_limited() {
        assert_eq!(clamp_steps(MediaAction::VolumeUp, Some(500)), MAX_VOLUME_STEPS);
        assert_eq!(clamp_steps(MediaAction::VolumeDown, Some(-3)), 1);
        assert_eq!(clamp_steps(MediaAction::VolumeUp, None), 5);
        assert_eq!(clamp_steps(MediaAction::Next, Some(9)), 1);
        assert_eq!(key_sequence(MediaAction::VolumeUp, 3), vec![0xAF, 0xAF, 0xAF]);
        assert_eq!(key_sequence(MediaAction::PlayPause, 1), vec![0xB3]);
        // Todas las teclas son multimedia/volumen (0xAD–0xB3), nunca letras.
        for n in MediaAction::NAMES {
            let vk = MediaAction::parse(n).unwrap().vk();
            assert!((0xAD..=0xB3).contains(&vk));
        }
    }
}
