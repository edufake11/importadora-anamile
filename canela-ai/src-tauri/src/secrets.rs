//! Almacenamiento seguro de claves API en el llavero del sistema operativo.
//!
//! - Windows: Administrador de credenciales (Credential Manager), cifrado con
//!   la sesión del usuario mediante DPAPI.
//! - macOS: Keychain. Linux: Secret Service (GNOME Keyring / KWallet).
//!
//! Las claves sólo se leen dentro del proceso Rust, justo antes de llamar a la
//! API. La interfaz (webview) nunca recibe el valor: sólo sabe si existe.

use crate::settings::ProviderId;
use keyring::Entry;

const SERVICE: &str = "com.canela.ai";

fn entry(provider: ProviderId) -> Result<Entry, String> {
    Entry::new(SERVICE, &format!("{}-api-key", provider.as_str())).map_err(|e| format!("No se pudo acceder al llavero del sistema: {e}"))
}

pub fn set_key(provider: ProviderId, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.len() < 20 || key.chars().any(char::is_whitespace) {
        return Err("La clave no parece válida (demasiado corta o con espacios).".into());
    }
    entry(provider)?
        .set_password(key)
        .map_err(|e| format!("No se pudo guardar la clave en el llavero: {e}"))
}

pub fn get_key(provider: ProviderId) -> Result<Option<String>, String> {
    match entry(provider)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("No se pudo leer la clave del llavero: {e}")),
    }
}

pub fn delete_key(provider: ProviderId) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("No se pudo eliminar la clave: {e}")),
    }
}

pub fn has_key(provider: ProviderId) -> bool {
    matches!(get_key(provider), Ok(Some(_)))
}
