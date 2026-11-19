//! Enrutador de modelos (v0.6, en desarrollo).
//!
//! Decide, para cada mensaje, qué modelo conviene usar, con reglas
//! explicables y sin llamar a ninguna IA para decidir:
//!
//! 1. Privacidad: datos sensibles → sólo el modelo local (nunca a internet).
//! 2. Capacidad: imágenes que el modelo local no ve → proponer uno externo.
//! 3. Disponibilidad: Ollama apagado → proponer uno externo.
//! 4. Complejidad: lo sencillo se queda en local; lo complejo se queda en local
//!    pero se PROPONE un modelo externo con su costo estimado.
//! 5. Gasto: nunca se propone ni se usa un modelo de pago por encima del
//!    límite mensual.
//!
//! Un modelo externo sólo se usa con permiso explícito (tarjeta en el chat).
//! Un solo modelo por mensaje: nunca se consultan varios a la vez.

use crate::ai::pricing::{self, Usage};
use crate::settings::{ProviderId, Settings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Simple,
    Medium,
    Complex,
}

/// Lo que se sabe del mensaje antes de enviarlo.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Request {
    pub text: String,
    pub has_images: bool,
    /// Caracteres de archivos adjuntos (ya incluidos en `text` o aparte).
    pub attachment_chars: usize,
    /// Tamaño del historial que se reenviaría.
    pub history_chars: usize,
}

/// Lo que se sabe del equipo y de tus ajustes.
#[derive(Debug, Clone)]
pub struct Context {
    pub main: ProviderId,
    /// «Proponer Claude/ChatGPT para tareas difíciles».
    pub auto: bool,
    pub local_ok: bool,
    pub local_vision: bool,
    /// Modelo externo preferido con clave guardada (si hay).
    pub cloud: Option<(ProviderId, String)>,
    pub spent_month_usd: f64,
    pub budget_usd: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudOffer {
    pub provider: ProviderId,
    pub model: String,
    /// «Claude» / «ChatGPT».
    pub label: String,
    /// Costo estimado de esta respuesta (None = precio desconocido).
    pub est_cost_usd: Option<f64>,
    /// Por qué se propone, en una frase.
    pub why: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// Modelo que se usará si no eliges otra cosa.
    pub provider: ProviderId,
    pub complexity: Complexity,
    /// Motivos detectados (para explicar la decisión).
    pub reasons: Vec<String>,
    /// Alternativa externa que requiere tu permiso.
    pub offer: Option<CloudOffer>,
    /// Si no se puede continuar, el motivo (y `provider` no se debe usar).
    pub blocked: Option<String>,
    /// El mensaje parece contener datos privados: nunca sale del equipo.
    pub private: bool,
    pub spent_month_usd: f64,
    pub budget_usd: f64,
}

// ------------------------------------------------------------ complejidad

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

/// Palabras que suelen indicar trabajo de razonamiento largo.
const HEAVY: [&str; 34] = [
    "analiza",
    "analisis",
    "compara",
    "comparacion",
    "diseña",
    "disena",
    "arquitectura",
    "investiga",
    "demuestra",
    "optimiza",
    "depura",
    "debug",
    "refactoriza",
    "refactor",
    "en detalle",
    "detalladamente",
    "paso a paso",
    "estrategia",
    "plan de negocio",
    "evalua",
    "ventajas y desventajas",
    "pros y contras",
    "ensayo",
    "informe",
    "implementa",
    "algoritmo",
    "complejidad",
    "por que falla",
    "traceback",
    "stack trace",
    "excepcion",
    "error:",
    "migra",
    "revisa este codigo",
];

/// Peticiones que piden elaborar algo (respuesta normal, no trivial).
const MEDIUM: [&str; 14] = [
    "explica",
    "escribe",
    "redacta",
    "resume",
    "como instalo",
    "como hago",
    "como puedo",
    "diferencia",
    "ideas",
    "funcion",
    "correo",
    "por que ",
    "ejemplo",
    "detallado",
];

/// Peticiones cortas y directas (acciones, saludos).
const LIGHT: [&str; 22] = [
    "hola",
    "gracias",
    "buenos dias",
    "buenas noches",
    "abre",
    "cierra",
    "pon ",
    "pausa",
    "siguiente cancion",
    "sube el volumen",
    "baja el volumen",
    "recuerdame",
    "recuerda que",
    "crea una tarea",
    "agrega",
    "que hora",
    "que tengo",
    "pendientes",
    "marca",
    "busca el archivo",
    "como estas",
    "chiste",
];

pub fn complexity(req: &Request) -> (Complexity, Vec<String>) {
    let t = fold(&req.text);
    let words = t.split_whitespace().count();
    let mut score: i32 = 0;
    let mut why = Vec::new();

    if words > 120 {
        score += 2;
        why.push("mensaje largo".to_string());
    } else if words > 45 {
        score += 1;
        why.push("mensaje de varias frases".to_string());
    }
    let code_lines = req
        .text
        .lines()
        .filter(|l| {
            let l = l.trim();
            l.ends_with(';')
                || l.ends_with('{')
                || l.ends_with('}')
                || l.starts_with("def ")
                || l.starts_with("fn ")
                || l.starts_with("import ")
        })
        .count();
    if t.contains("```") || code_lines >= 3 {
        score += 2;
        why.push("incluye código".to_string());
    }
    let heavy: Vec<&str> = HEAVY.iter().copied().filter(|k| t.contains(k)).collect();
    if !heavy.is_empty() {
        // Una señal de razonamiento pesa 2; varias, 3.
        score += if heavy.len() == 1 { 2 } else { 3 };
        why.push(format!("pide {}", heavy.iter().take(2).copied().collect::<Vec<_>>().join(" y ")));
    }
    if MEDIUM.iter().any(|k| t.contains(k)) {
        score += 1;
    }
    let questions = req.text.matches('?').count();
    if questions >= 3 {
        score += 1;
        why.push("varias preguntas".to_string());
    }
    let numbered = req
        .text
        .lines()
        .filter(|l| {
            let l = l.trim_start();
            l.chars().next().is_some_and(|c| c.is_ascii_digit()) && (l.contains(". ") || l.contains(") "))
        })
        .count();
    if numbered >= 3 {
        score += 1;
        why.push("lista de requisitos".to_string());
    }
    if req.attachment_chars > 8_000 {
        score += 1;
        why.push("adjunto grande".to_string());
    }
    if req.history_chars > 60_000 {
        score += 1;
        why.push("conversación muy larga".to_string());
    }
    if LIGHT.iter().any(|k| t.contains(k)) && words < 25 && heavy.is_empty() {
        score -= 1;
    }

    let level = if score >= 3 {
        Complexity::Complex
    } else if score <= 0 && words < 30 {
        Complexity::Simple
    } else {
        Complexity::Medium
    };
    (level, why)
}

// ------------------------------------------------------------ privacidad

/// ¿Datos que nunca deben salir del equipo?
pub fn private_reason(text: &str) -> Option<&'static str> {
    if let Some(r) = crate::db::looks_sensitive(text) {
        return Some(r);
    }
    // 13–19 cifras seguidas (se permiten espacios o guiones entre ellas):
    // posible número de tarjeta o de cuenta.
    let (mut run, mut longest) = (0usize, 0usize);
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
            longest = longest.max(run);
        } else if c != ' ' && c != '-' {
            run = 0;
        }
    }
    if (13..=19).contains(&longest) {
        return Some("un número de tarjeta o cuenta");
    }
    let up = text.to_uppercase();
    if up.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
        w.len() >= 15
            && w.len() <= 34
            && w.as_bytes()[..2].iter().all(u8::is_ascii_alphabetic)
            && w[2..].chars().all(|c| c.is_ascii_digit())
    }) {
        return Some("un número de cuenta (IBAN)");
    }
    let f = fold(text);
    for (k, why) in [
        ("curp", "un documento de identidad"),
        ("rfc ", "un documento de identidad"),
        ("dni ", "un documento de identidad"),
        ("pasaporte", "un documento de identidad"),
        ("historia clinica", "datos de salud"),
        ("diagnostico medico", "datos de salud"),
    ] {
        if f.contains(k) {
            return Some(why);
        }
    }
    None
}

// ------------------------------------------------------------ costo

pub fn provider_label(p: ProviderId) -> &'static str {
    match p {
        ProviderId::Anthropic => "Claude",
        ProviderId::Openai => "ChatGPT",
        ProviderId::Ollama => "el modelo local",
    }
}

/// Instrucciones + herramientas ≈ 5 200 tokens (medido en v0.4).
const BASE_PROMPT_TOKENS: u64 = 5_200;

/// Costo estimado de responder (aprox.: 1 token ≈ 3,2 caracteres en español).
pub fn estimate_cost(provider: ProviderId, model: &str, req: &Request, level: Complexity) -> Option<f64> {
    let price = pricing::price(provider, model)?;
    let chars = req.text.chars().count() + req.attachment_chars + req.history_chars;
    let input = BASE_PROMPT_TOKENS + (chars as u64 * 10 / 32);
    let output = match level {
        Complexity::Simple => 300,
        Complexity::Medium => 900,
        Complexity::Complex => 2_500,
    };
    Some(pricing::cost(
        price,
        &Usage {
            input,
            output,
            ..Default::default()
        },
    ))
}

// ------------------------------------------------------------ política

pub fn decide(req: &Request, ctx: &Context) -> Decision {
    let (level, mut reasons) = complexity(req);
    let private = private_reason(&req.text);
    let base = |provider: ProviderId, reasons: Vec<String>| Decision {
        provider,
        complexity: level,
        reasons,
        offer: None,
        blocked: None,
        private: private.is_some(),
        spent_month_usd: ctx.spent_month_usd,
        budget_usd: ctx.budget_usd,
    };
    let over_budget =
        |extra: f64| ctx.budget_usd <= 0.0 || ctx.spent_month_usd >= ctx.budget_usd || ctx.spent_month_usd + extra > ctx.budget_usd;

    // Proveedor principal externo: siempre ése, dentro del presupuesto.
    if ctx.main != ProviderId::Ollama {
        let mut d = base(ctx.main, reasons);
        if let Some(why) = private {
            // Ya elegiste un modelo externo como principal: se avisa, no se bloquea.
            d.reasons
                .push(format!("contiene {} (se enviará a {})", why, provider_label(ctx.main)));
        }
        if over_budget(0.0) {
            d.blocked = Some(format!(
                "Alcanzaste tu límite de gasto mensual ({:.2} US$ de {:.2} US$). Súbelo en Ajustes › Asistente o usa el modelo local.",
                ctx.spent_month_usd, ctx.budget_usd
            ));
        }
        return d;
    }

    let needs_vision = req.has_images && !ctx.local_vision;
    let mut d = base(ProviderId::Ollama, reasons.clone());

    if let Some(why) = private {
        d.reasons.push(format!("contiene {why}: sólo se usa el modelo local"));
        if !ctx.local_ok {
            d.blocked = Some(format!(
                "El mensaje parece contener {why} y el modelo local (Ollama) no está disponible. Por tu privacidad no lo enviaré a internet: abre Ollama y vuelve a intentarlo."
            ));
        } else if needs_vision {
            d.blocked = Some(format!(
                "El mensaje parece contener {why} y el modelo local no ve imágenes. Por tu privacidad no lo enviaré a internet."
            ));
        }
        return d;
    }

    // Motivo para proponer un modelo externo.
    let why = if !ctx.local_ok {
        Some("el modelo local (Ollama) no está disponible".to_string())
    } else if needs_vision {
        Some("el modelo local no puede ver imágenes".to_string())
    } else if ctx.auto && level == Complexity::Complex {
        Some(format!("la tarea parece compleja ({})", reasons.join(", ")))
    } else {
        None
    };
    let Some(why) = why else { return d };

    let offer = ctx.cloud.as_ref().and_then(|(p, model)| {
        let est = estimate_cost(*p, model, req, level);
        if over_budget(est.unwrap_or(0.0)) {
            reasons.push("límite de gasto mensual alcanzado".into());
            return None;
        }
        Some(CloudOffer {
            provider: *p,
            model: model.clone(),
            label: provider_label(*p).to_string(),
            est_cost_usd: est,
            why: why.clone(),
        })
    });
    d.reasons = reasons;
    if !ctx.local_ok || needs_vision {
        // Sin alternativa local útil: o se acepta el externo o no se puede.
        if offer.is_none() {
            d.blocked = Some(if needs_vision && ctx.local_ok {
                "El modelo local no puede ver imágenes y no hay un modelo externo disponible (falta la clave o se alcanzó el límite de gasto).".into()
            } else {
                crate::ai::ollama::connect_error_message(&Settings::default())
            });
        }
    }
    d.offer = offer;
    d
}

// ------------------------------------------------------------ gasto

/// ¿Se puede usar este proveedor externo ahora? (control final en el backend)
pub fn check_cloud_use(provider: ProviderId, text: &str, spent_month: f64, budget: f64) -> Result<(), String> {
    if provider == ProviderId::Ollama {
        return Ok(());
    }
    if budget <= 0.0 || spent_month >= budget {
        return Err(format!(
            "Alcanzaste tu límite de gasto mensual ({spent_month:.2} US$ de {budget:.2} US$). Súbelo en Ajustes › Asistente o usa el modelo local."
        ));
    }
    if let Some(why) = private_reason(text) {
        return Err(format!(
            "El mensaje parece contener {why}: por tu privacidad no lo enviaré a {}.",
            provider_label(provider)
        ));
    }
    Ok(())
}

/// Clave «AAAA-MM» del mes actual (hora local).
pub fn month_key() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

// ------------------------------------------------------------ comandos

/// Apunta el costo de una respuesta de pago (sólo proveedores externos).
pub fn record_spend(db: &crate::db::Db, provider: ProviderId, model: &str, usage: &Usage) -> Option<f64> {
    if provider == ProviderId::Ollama {
        return None;
    }
    let c = pricing::price(provider, model).map(|p| pricing::cost(p, usage))?;
    let _ = db.spend_add(
        &month_key(),
        provider.as_str(),
        model,
        usage.input + usage.cache_read + usage.cache_write,
        usage.output,
        c,
    );
    Some(c)
}

/// Estado de Ollama en caché (20 s) para no retrasar cada mensaje.
static LOCAL_CACHE: std::sync::Mutex<Option<(std::time::Instant, String, bool, bool)>> = std::sync::Mutex::new(None);

async fn local_status(http: &reqwest::Client, settings: &Settings) -> (bool, bool) {
    let key = format!("{}|{}", settings.ollama_url, settings.ollama_model);
    if let Some((t, k, ok, vision)) = LOCAL_CACHE.lock().unwrap().clone() {
        if k == key && t.elapsed() < std::time::Duration::from_secs(20) {
            return (ok, vision);
        }
    }
    let st = crate::ai::ollama::status(http, settings).await;
    let ok = st.running && st.model_installed;
    let vision = st.capabilities.iter().any(|c| c == "vision")
        || crate::ai::model_supports_images(ProviderId::Ollama, &settings.ollama_model) == Some(true);
    *LOCAL_CACHE.lock().unwrap() = Some((std::time::Instant::now(), key, ok, vision));
    (ok, vision)
}

fn has_key(p: ProviderId) -> bool {
    #[cfg(debug_assertions)]
    if std::env::var("CANELA_DEV_API_KEY").is_ok() {
        return true;
    }
    crate::secrets::has_key(p)
}

/// Modelo externo preferido que tenga clave guardada.
fn cloud_choice(settings: &Settings) -> Option<(ProviderId, String)> {
    let model = |p: ProviderId| match p {
        ProviderId::Anthropic => settings.anthropic_model.clone(),
        _ => settings.openai_model.clone(),
    };
    let first = settings.cloud_preference;
    let second = if first == ProviderId::Anthropic {
        ProviderId::Openai
    } else {
        ProviderId::Anthropic
    };
    [first, second].into_iter().find(|p| has_key(*p)).map(|p| (p, model(p)))
}

/// Qué modelo usar para este mensaje (no envía nada a ninguna IA).
#[tauri::command]
pub async fn route_decide(state: tauri::State<'_, crate::commands::AppState>, request: Request) -> Result<Decision, String> {
    let settings = state.current_settings();
    let (local_ok, local_vision) = if settings.provider == ProviderId::Ollama {
        local_status(&state.http, &settings).await
    } else {
        (false, false)
    };
    let s2 = settings.clone();
    let cloud = tauri::async_runtime::spawn_blocking(move || cloud_choice(&s2))
        .await
        .map_err(|e| e.to_string())?;
    let ctx = Context {
        main: settings.provider,
        auto: settings.router_auto,
        local_ok,
        local_vision,
        cloud,
        spent_month_usd: state.db.spend_month(&month_key()).unwrap_or(0.0),
        budget_usd: settings.monthly_budget_usd,
    };
    Ok(decide(&request, &ctx))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendSummary {
    pub month: String,
    pub spent_usd: f64,
    pub budget_usd: f64,
    pub breakdown: Vec<crate::db::SpendRow>,
}

#[tauri::command]
pub fn spend_summary(state: tauri::State<'_, crate::commands::AppState>) -> Result<SpendSummary, String> {
    let month = month_key();
    Ok(SpendSummary {
        spent_usd: state.db.spend_month(&month)?,
        budget_usd: state.current_settings().monthly_budget_usd,
        breakdown: state.db.spend_breakdown(&month)?,
        month,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(t: &str) -> Request {
        Request {
            text: t.into(),
            ..Default::default()
        }
    }

    fn ctx() -> Context {
        Context {
            main: ProviderId::Ollama,
            auto: true,
            local_ok: true,
            local_vision: false,
            cloud: Some((ProviderId::Anthropic, "claude-opus-5-5".into())),
            spent_month_usd: 0.0,
            budget_usd: 5.0,
        }
    }

    /// Conjunto etiquetado a mano: mide la exactitud del clasificador.
    const LABELED: &[(&str, Complexity)] = &[
        ("hola canela", Complexity::Simple),
        ("¿qué tengo pendiente hoy?", Complexity::Simple),
        ("abre la calculadora", Complexity::Simple),
        ("pausa la música", Complexity::Simple),
        ("recuérdame llamar a mamá a las 6", Complexity::Simple),
        ("crea una tarea para estudiar mañana", Complexity::Simple),
        ("gracias, eres genial", Complexity::Simple),
        ("¿qué hora es?", Complexity::Simple),
        ("busca el archivo del contrato", Complexity::Simple),
        ("cuéntame un chiste", Complexity::Simple),
        ("sube el volumen", Complexity::Simple),
        ("buenos días canela", Complexity::Simple),
        ("¿cuál es la capital de Francia?", Complexity::Simple),
        ("¿qué es una variable en programación?", Complexity::Simple),
        ("traduce 'buenos días' al inglés", Complexity::Simple),
        ("explícame qué es React y para qué sirve", Complexity::Medium),
        ("escribe una función en Python que invierta una cadena", Complexity::Medium),
        ("resume este párrafo en dos líneas: la economía creció un 3 % impulsada por las exportaciones y el consumo interno, aunque la inflación se mantuvo alta durante el trimestre", Complexity::Medium),
        ("dame ideas para el cumpleaños de mi hermana, le gusta la música y salir a caminar", Complexity::Medium),
        ("¿cómo instalo Node.js en Windows 11?", Complexity::Medium),
        ("redacta un correo formal pidiendo una reunión con mi profesor para revisar mi proyecto final", Complexity::Medium),
        ("¿qué diferencia hay entre let y const en JavaScript?", Complexity::Medium),
        ("organiza mis descargas por tipo de archivo", Complexity::Simple),
        ("analiza la arquitectura de mi proyecto y propón una estrategia de refactorización paso a paso", Complexity::Complex),
        ("compara React, Vue y Svelte: ventajas y desventajas, rendimiento y curva de aprendizaje", Complexity::Complex),
        ("depura este error:\nTraceback (most recent call last):\n  File \"app.py\", line 3\n    print(x\nSyntaxError", Complexity::Complex),
        ("revisa este código y optimiza su complejidad:\n```python\nfor i in range(n):\n    for j in range(n):\n        total += a[i]*b[j]\n```", Complexity::Complex),
        ("diseña la arquitectura de una app de tareas con sincronización offline y explica en detalle cada componente", Complexity::Complex),
        ("escribe un informe detallado sobre el impacto de la inteligencia artificial en la educación con pros y contras", Complexity::Complex),
        ("implementa un algoritmo de Dijkstra en Rust y analiza su complejidad", Complexity::Complex),
        ("haz un plan de negocio para una cafetería: costos, estrategia de marketing y proyección a un año", Complexity::Complex),
        ("evalúa estas tres opciones de laptop para programar y dime cuál conviene más y por qué\n1. Lenovo\n2. Asus\n3. Dell", Complexity::Complex),
        ("¿por qué falla mi build? error: cannot find module 'react' — revisa este código del package.json y la configuración de webpack", Complexity::Complex),
        ("migra este proyecto de JavaScript a TypeScript explicando paso a paso los cambios", Complexity::Complex),
        ("investiga y compara los modelos de lenguaje locales que caben en 16 GB de VRAM", Complexity::Complex),
    ];

    #[test]
    fn classifier_accuracy_on_labeled_set() {
        let mut ok = 0;
        let mut misses = Vec::new();
        for (text, expected) in LABELED {
            let (got, _) = complexity(&req(text));
            if got == *expected {
                ok += 1;
            } else {
                misses.push(format!("{text:?}: esperado {expected:?}, obtenido {got:?}"));
            }
        }
        let acc = ok as f64 / LABELED.len() as f64;
        eprintln!("Exactitud del clasificador: {ok}/{} = {:.0} %", LABELED.len(), acc * 100.0);
        for m in &misses {
            eprintln!("  fallo: {m}");
        }
        assert!(acc >= 0.85, "exactitud {:.0} %: {misses:#?}", acc * 100.0);
        // Lo que importa más: ninguna tarea sencilla se clasifica como compleja
        // (eso propondría gastar dinero sin necesidad).
        for (text, expected) in LABELED {
            if *expected == Complexity::Simple {
                assert_ne!(complexity(&req(text)).0, Complexity::Complex, "{text}");
            }
        }
    }

    /// Conjunto NO usado para ajustar las reglas (escrito antes de medirlo):
    /// da una idea más honesta de la exactitud con mensajes nuevos.
    const HELD_OUT: &[(&str, Complexity)] = &[
        ("pon música relajante", Complexity::Simple),
        ("¿cuántos días faltan para navidad?", Complexity::Simple),
        ("marca la tarea de inglés como hecha", Complexity::Simple),
        ("baja el volumen un poco", Complexity::Simple),
        ("¿cómo estás hoy?", Complexity::Simple),
        ("abre mi carpeta de la universidad", Complexity::Simple),
        ("¿qué significa la palabra efímero?", Complexity::Simple),
        ("explícame la fotosíntesis de forma sencilla", Complexity::Medium),
        ("escribe un poema corto sobre una perrita salchicha", Complexity::Medium),
        ("¿cómo puedo cambiar el fondo de pantalla en Windows 11?", Complexity::Medium),
        ("redacta un mensaje para felicitar a mi jefe por su ascenso", Complexity::Medium),
        ("dame un ejemplo de bucle for en JavaScript", Complexity::Medium),
        ("resume en tres puntos qué es la inflación", Complexity::Medium),
        ("analiza estos datos de ventas y detecta tendencias, estacionalidad y anomalías por trimestre", Complexity::Complex),
        ("diseña un algoritmo para repartir turnos de trabajo de forma justa entre 12 personas y explica su complejidad", Complexity::Complex),
        ("refactoriza este código para que sea más legible:\n```js\nfunction f(a){let r=[];for(let i=0;i<a.length;i++){if(a[i]%2==0){r.push(a[i]*2)}}return r}\n```", Complexity::Complex),
        ("compara tres estrategias de inversión para principiantes con sus riesgos, ventajas y desventajas", Complexity::Complex),
        ("investiga cómo funciona la memoria virtual en Windows y explica en detalle la memoria comprometida", Complexity::Complex),
        ("¿por qué falla esta consulta SQL? error: column \"fecha\" does not exist. Revisa este código y propón la corrección", Complexity::Complex),
        ("prepara un informe paso a paso para migrar mi web de PHP a Node.js", Complexity::Complex),
    ];

    #[test]
    fn classifier_accuracy_on_held_out_set() {
        let mut ok = 0;
        for (text, expected) in HELD_OUT {
            let (got, _) = complexity(&req(text));
            if got == *expected {
                ok += 1;
            } else {
                eprintln!("  fallo (no ajustado): {text:?}: esperado {expected:?}, obtenido {got:?}");
            }
        }
        let acc = ok as f64 / HELD_OUT.len() as f64;
        eprintln!("Exactitud con mensajes nuevos: {ok}/{} = {:.0} %", HELD_OUT.len(), acc * 100.0);
        // Nunca proponer gastar en tareas sencillas.
        for (text, expected) in HELD_OUT {
            if *expected == Complexity::Simple {
                assert_ne!(complexity(&req(text)).0, Complexity::Complex, "{text}");
            }
        }
        assert!(acc >= 0.7, "exactitud {:.0} %", acc * 100.0);
    }

    #[test]
    fn simple_stays_local_without_asking() {
        let d = decide(&req("abre la calculadora"), &ctx());
        assert_eq!(d.provider, ProviderId::Ollama);
        assert!(d.offer.is_none() && d.blocked.is_none());
    }

    #[test]
    fn complex_stays_local_but_offers_cloud_with_cost() {
        let d = decide(&req(LABELED[23].0), &ctx());
        assert_eq!(d.complexity, Complexity::Complex);
        assert_eq!(d.provider, ProviderId::Ollama, "por defecto sigue en local");
        let o = d.offer.expect("propone Claude");
        assert_eq!(o.provider, ProviderId::Anthropic);
        assert_eq!(o.label, "Claude");
        let c = o.est_cost_usd.unwrap();
        assert!(c > 0.0 && c < 0.2, "{c}");
        assert!(o.why.contains("compleja"));
        // Sin «proponer modelos externos»: nunca se ofrece.
        let mut c2 = ctx();
        c2.auto = false;
        assert!(decide(&req(LABELED[23].0), &c2).offer.is_none());
    }

    #[test]
    fn private_data_never_leaves_the_computer() {
        let text = "analiza en detalle por qué falla mi login, mi contraseña es Perro123 y uso este algoritmo de cifrado";
        let d = decide(&req(text), &ctx());
        assert!(d.private);
        assert_eq!(d.provider, ProviderId::Ollama);
        assert!(d.offer.is_none(), "aunque sea compleja, no se propone internet");
        // Ollama apagado + datos privados: se bloquea, no se envía fuera.
        let mut c = ctx();
        c.local_ok = false;
        let d = decide(&req(text), &c);
        assert!(d.offer.is_none());
        assert!(d.blocked.unwrap().contains("no lo enviaré a internet"));
        assert!(private_reason("mi tarjeta es 4111 1111 1111 1111").is_some());
        assert!(private_reason("transfiere a ES9121000418450200051332").is_some());
        assert!(private_reason("mi CURP es XXXX").is_some());
        assert!(private_reason("tengo 3 tareas para el 2026").is_none());
        assert!(private_reason("llama al 555 1234").is_none());
    }

    #[test]
    fn images_and_ollama_down_offer_cloud() {
        let mut r = req("¿qué ves en esta imagen?");
        r.has_images = true;
        let d = decide(&r, &ctx());
        assert!(d.offer.as_ref().unwrap().why.contains("no puede ver imágenes"));
        let mut c = ctx();
        c.local_vision = true;
        assert!(decide(&r, &c).offer.is_none(), "con un modelo local de visión no hace falta");
        let mut c = ctx();
        c.local_ok = false;
        let d = decide(&req("hola"), &c);
        assert!(d.offer.unwrap().why.contains("no está disponible"));
        assert!(d.blocked.is_none(), "puedes aceptar el externo");
        // Sin clave externa: no hay alternativa.
        let mut c = ctx();
        c.local_ok = false;
        c.cloud = None;
        assert!(decide(&req("hola"), &c).blocked.unwrap().contains("Ollama"));
    }

    #[test]
    fn budget_limits_offers_and_main_cloud_provider() {
        let mut c = ctx();
        c.spent_month_usd = 4.99;
        let d = decide(&req(LABELED[24].0), &c);
        assert!(d.offer.is_none(), "el costo estimado superaría el límite");
        assert!(d.reasons.iter().any(|r| r.contains("límite")));
        let mut c = ctx();
        c.main = ProviderId::Anthropic;
        c.spent_month_usd = 5.0;
        assert!(decide(&req("hola"), &c).blocked.unwrap().contains("límite de gasto"));
        c.spent_month_usd = 1.0;
        let d = decide(&req("hola"), &c);
        assert_eq!(d.provider, ProviderId::Anthropic);
        assert!(d.blocked.is_none());
        // Presupuesto 0 = no gastar nada en modelos de pago.
        let mut c = ctx();
        c.budget_usd = 0.0;
        assert!(decide(&req(LABELED[24].0), &c).offer.is_none());
    }

    #[test]
    fn backend_check_blocks_over_budget_or_private() {
        assert!(check_cloud_use(ProviderId::Ollama, "mi contraseña es x", 99.0, 0.0).is_ok());
        assert!(check_cloud_use(ProviderId::Anthropic, "hola", 1.0, 5.0).is_ok());
        assert!(check_cloud_use(ProviderId::Anthropic, "hola", 5.0, 5.0)
            .unwrap_err()
            .contains("límite"));
        assert!(check_cloud_use(ProviderId::Openai, "mi contraseña es x", 0.0, 5.0)
            .unwrap_err()
            .contains("privacidad"));
    }

    #[test]
    fn cost_estimate_grows_with_context_and_unknown_models_have_none() {
        let small = estimate_cost(ProviderId::Anthropic, "claude-opus-5-5", &req("hola"), Complexity::Simple).unwrap();
        let mut big = req("hola");
        big.history_chars = 100_000;
        let large = estimate_cost(ProviderId::Anthropic, "claude-opus-5-5", &big, Complexity::Complex).unwrap();
        assert!(large > small * 5.0, "{small} → {large}");
        assert!(estimate_cost(ProviderId::Openai, "modelo-inventado", &req("hola"), Complexity::Simple).is_none());
        assert_eq!(
            estimate_cost(ProviderId::Ollama, "qwen3:8b", &req("hola"), Complexity::Simple),
            Some(0.0)
        );
    }
}
