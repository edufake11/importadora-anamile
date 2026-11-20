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
    /// Contenido de los adjuntos (para revisar la privacidad).
    pub attachments: Vec<String>,
    /// Textos anteriores de la conversación (para revisar la privacidad).
    pub history: Vec<String>,
    /// Imágenes en mensajes anteriores.
    pub history_images: usize,
}

/// Un modelo disponible.
#[derive(Debug, Clone)]
pub struct Model {
    pub provider: ProviderId,
    pub model: String,
}

/// Lo que se sabe del equipo y de tus ajustes.
#[derive(Debug, Clone)]
pub struct Context {
    pub main: ProviderId,
    /// «Proponer Claude/ChatGPT para tareas difíciles».
    pub auto: bool,
    pub local_ok: bool,
    pub local_vision: bool,
    /// El modelo local admite herramientas.
    pub local_tools: bool,
    /// Contexto del modelo local (tokens).
    pub local_ctx_tokens: u32,
    /// El modelo local ya está en memoria (si no, tarda en cargar).
    pub local_loaded: bool,
    /// Modelos externos con clave guardada, en orden de preferencia.
    pub clouds: Vec<Model>,
    pub spent_month_usd: f64,
    /// Lo reservado por otras respuestas en curso.
    pub reserved_usd: f64,
    pub budget_usd: f64,
}

/// Un candidato evaluado (para explicar la elección).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub provider: ProviderId,
    pub model: String,
    pub label: String,
    pub eligible: bool,
    /// Por qué no sirve (si no es elegible).
    pub why_not: Option<String>,
    /// Calidad relativa esperada (1 = local pequeño, 3 = modelo grande externo).
    pub quality: u8,
    /// ESTIMACIONES (no son el costo ni el tiempo final).
    pub est_cost_usd: Option<f64>,
    pub est_seconds: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudOffer {
    pub provider: ProviderId,
    pub model: String,
    /// «Claude» / «ChatGPT».
    pub label: String,
    /// Costo ESTIMADO de esta respuesta (None = precio desconocido). El costo
    /// final depende de los tokens reales que informe el proveedor.
    pub est_cost_usd: Option<f64>,
    pub est_seconds: f64,
    /// Por qué se propone, en una frase.
    pub why: String,
    /// Tipos de datos que necesitan tu autorización explícita para enviarlos.
    pub ack: Vec<String>,
    /// Qué se detectó (para mostrarlo en la tarjeta).
    pub ack_summary: String,
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
    /// Hay secretos: nunca sale del equipo.
    pub private: bool,
    /// Revisión de privacidad de todo lo que se enviaría.
    pub privacy: crate::privacy::Report,
    /// Con un proveedor principal externo: datos que necesitan autorización.
    pub ack_required: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub spent_month_usd: f64,
    pub reserved_usd: f64,
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
const HEAVY: [&str; 40] = [
    "planifica",
    "investigo",
    "seguridad",
    "escalabilidad",
    "recomiendame",
    "cuello de botella",
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

/// Verbos de instrucción: dos o más distintos = tarea encadenada.
const ACTIONS: [&str; 24] = [
    "analiza",
    "propon",
    "explica",
    "compara",
    "recomienda",
    "justifica",
    "revisa",
    "disena",
    "implementa",
    "crea",
    "reescribe",
    "detalla",
    "senala",
    "evalua",
    "investiga",
    "resume",
    "escribe",
    "aplica",
    "optimiza",
    "depura",
    "migra",
    "planifica",
    "calcula",
    "prueba",
];

/// Señales de tarea extensa (alcance): planes, modelos, pruebas formales…
const SCOPE: [&str; 18] = [
    "plan de",
    "planifica",
    "modelo de negocio",
    "analisis financiero",
    "demuestra que",
    "contrato",
    "clausula",
    "completo",
    "completa",
    "a fondo",
    "exhaustiv",
    "cada ",
    "con argumentos",
    "semanas",
    "cronograma",
    "casos limite",
    "sin tiempo de inactividad",
    "fuga",
];

/// Peticiones que piden elaborar algo (respuesta normal, no trivial).
const MEDIUM: [&str; 22] = [
    "que es ",
    "que son",
    "como ",
    "cuales son",
    "consejos",
    "receta",
    "corrige",
    "traduce este",
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
    // Alcance: varias instrucciones distintas, requisitos enumerados, datos.
    let actions: Vec<&str> = ACTIONS
        .iter()
        .copied()
        .filter(|a| t.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(a)))
        .collect();
    if actions.len() >= 2 && (!heavy.is_empty() || words > 12) {
        score += 1;
        why.push("varias instrucciones encadenadas".to_string());
    }
    let scope: Vec<&str> = SCOPE.iter().copied().filter(|k| t.contains(k)).collect();
    if !scope.is_empty() {
        score += if scope
            .iter()
            .any(|k| ["demuestra que", "modelo de negocio", "analisis financiero"].contains(k))
        {
            3
        } else {
            1
        };
        why.push("pide un trabajo extenso".to_string());
    }
    let items = req.text.matches(',').count() + t.matches(" y ").count();
    if items >= 3 && words > 10 {
        score += 1;
        why.push("varios requisitos".to_string());
    }
    let numbers = req.text.split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).count();
    if numbers >= 3 && (!heavy.is_empty() || !scope.is_empty() || actions.len() >= 2) {
        score += 1;
        why.push("incluye datos para analizar".to_string());
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

/// Compatibilidad (pruebas): motivo si el texto contiene un SECRETO.
#[cfg(test)]
pub fn private_reason(text: &str) -> Option<String> {
    let mut f = Vec::new();
    crate::privacy::scan_text(text, "mensaje", &mut f);
    f.into_iter().find(|x| x.level == crate::privacy::Level::Secret).map(|x| x.what)
}

/// ¿Pide una acción que requiere herramientas (abrir, crear tareas…)?
pub fn needs_tools(text: &str) -> bool {
    let t = fold(text);
    const TOOLS: [&str; 22] = [
        "abre ",
        "abrir ",
        "cierra ",
        "crea una tarea",
        "agrega",
        "recuerdame",
        "recuerda que",
        "marca ",
        "borra ",
        "elimina ",
        "pausa",
        "siguiente cancion",
        "volumen",
        "busca el archivo",
        "busca en mis",
        "organiza",
        "captura",
        "lee el",
        "mis tareas",
        "pendiente",
        "archivos recientes",
        "espacio libre",
    ];
    TOOLS.iter().any(|k| t.contains(k))
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

fn input_tokens(req: &Request) -> u64 {
    let chars = req.text.chars().count() + req.attachment_chars + req.history_chars;
    BASE_PROMPT_TOKENS + (chars as u64 * 10 / 32)
}

fn output_tokens(level: Complexity) -> u64 {
    match level {
        Complexity::Simple => 300,
        Complexity::Medium => 900,
        Complexity::Complex => 2_500,
    }
}

/// Costo ESTIMADO de responder (aprox.: 1 token ≈ 3,2 caracteres en español;
/// salida según complejidad). El costo real se calcula después con los
/// tokens que informa el proveedor.
pub fn estimate_cost(provider: ProviderId, model: &str, req: &Request, level: Complexity) -> Option<f64> {
    let price = pricing::price(provider, model)?;
    Some(pricing::cost(
        price,
        &Usage {
            input: input_tokens(req),
            output: output_tokens(level),
            ..Default::default()
        },
    ))
}

/// Tiempo ESTIMADO (segundos). Local: carga del modelo si no está en
/// memoria + lectura del contexto + escritura (≈ 35 tokens/s, típico de un
/// modelo de 8B en una GPU de gama media). Externo: latencia + ≈ 60 tokens/s.
pub fn estimate_seconds(provider: ProviderId, req: &Request, level: Complexity, local_loaded: bool) -> f64 {
    let (inp, out) = (input_tokens(req) as f64, output_tokens(level) as f64);
    match provider {
        ProviderId::Ollama => (if local_loaded { 0.0 } else { 10.0 }) + inp / 1_500.0 + out / 35.0,
        _ => 1.5 + inp / 6_000.0 + out / 60.0,
    }
}

// ------------------------------------------------------------ política

pub fn decide(req: &Request, ctx: &Context) -> Decision {
    use crate::privacy::{scan, Level, Outgoing};
    let (level, mut reasons) = complexity(req);
    let privacy = scan(&Outgoing {
        text: req.text.clone(),
        attachments: req.attachments.clone(),
        history: req.history.clone(),
        images: usize::from(req.has_images) + req.history_images,
    });
    let secret = privacy.level() == Level::Secret;
    let available = ctx.budget_usd - ctx.spent_month_usd - ctx.reserved_usd;
    let tools = needs_tools(&req.text);
    let tokens = input_tokens(req);

    // --- Candidatos
    let mut candidates = Vec::new();
    let local_why_not = if !ctx.local_ok {
        Some("Ollama no está disponible".to_string())
    } else if req.has_images && !ctx.local_vision {
        Some("no puede ver imágenes".to_string())
    } else if tools && !ctx.local_tools {
        Some("no admite herramientas".to_string())
    } else if tokens as f64 > ctx.local_ctx_tokens as f64 * 0.9 {
        Some(format!(
            "la conversación (≈ {} mil tokens) no cabe en su memoria de {} mil",
            tokens / 1000,
            ctx.local_ctx_tokens / 1000
        ))
    } else {
        None
    };
    candidates.push(Candidate {
        provider: ProviderId::Ollama,
        model: "local".into(),
        label: "Modelo local".into(),
        eligible: local_why_not.is_none(),
        why_not: local_why_not.clone(),
        quality: 1,
        est_cost_usd: Some(0.0),
        est_seconds: estimate_seconds(ProviderId::Ollama, req, level, ctx.local_loaded),
    });
    for m in &ctx.clouds {
        let est = estimate_cost(m.provider, &m.model, req, level);
        let why_not = if secret {
            Some(format!("contiene {}", privacy.findings[0].what))
        } else if ctx.budget_usd <= 0.0 || available <= 0.0 || est.is_some_and(|c| c > available) {
            Some("superaría tu límite de gasto mensual".to_string())
        } else {
            None
        };
        candidates.push(Candidate {
            provider: m.provider,
            model: m.model.clone(),
            label: provider_label(m.provider).into(),
            eligible: why_not.is_none(),
            why_not,
            quality: 3,
            est_cost_usd: est,
            est_seconds: estimate_seconds(m.provider, req, level, true),
        });
    }
    let local = candidates[0].clone();
    let best_cloud = candidates.iter().skip(1).find(|c| c.eligible).cloned();
    if candidates
        .iter()
        .skip(1)
        .any(|c| c.why_not.as_deref() == Some("superaría tu límite de gasto mensual"))
    {
        reasons.push("límite de gasto mensual alcanzado".into());
    }
    let ack = privacy.kinds_needing_ack();
    let mut d = Decision {
        provider: ctx.main,
        complexity: level,
        reasons,
        offer: None,
        blocked: None,
        private: secret,
        privacy: privacy.clone(),
        ack_required: Vec::new(),
        candidates: candidates.clone(),
        spent_month_usd: ctx.spent_month_usd,
        reserved_usd: ctx.reserved_usd,
        budget_usd: ctx.budget_usd,
    };
    let offer_from = |c: &Candidate, why: String| CloudOffer {
        provider: c.provider,
        model: c.model.clone(),
        label: c.label.clone(),
        est_cost_usd: c.est_cost_usd,
        est_seconds: c.est_seconds,
        why,
        ack: ack.clone(),
        ack_summary: privacy.summary(),
    };

    // --- Proveedor principal externo (elegiste Claude o ChatGPT)
    if ctx.main != ProviderId::Ollama {
        if secret {
            if local.eligible {
                d.provider = ProviderId::Ollama;
                d.reasons.push(format!(
                    "contiene {}: esta vez respondo con el modelo local",
                    privacy.findings[0].what
                ));
            } else {
                d.blocked = Some(format!(
                    "Por tu privacidad no enviaré esto a {}: contiene {}. Quita esos datos o abre Ollama para responder en local.",
                    provider_label(ctx.main),
                    privacy.summary()
                ));
            }
        } else if ctx.budget_usd <= 0.0 || available <= 0.0 {
            d.blocked = Some(format!(
                "Alcanzaste tu límite de gasto mensual ({:.2} US$ de {:.2} US$). Súbelo en Ajustes › Asistente o usa el modelo local.",
                ctx.spent_month_usd + ctx.reserved_usd,
                ctx.budget_usd
            ));
        } else {
            d.ack_required = ack.clone();
        }
        return d;
    }

    // --- Principal local
    d.provider = ProviderId::Ollama;
    if secret {
        d.reasons
            .push(format!("contiene {}: sólo se usa el modelo local", privacy.findings[0].what));
        if !local.eligible {
            d.blocked = Some(format!(
                "Contiene {} y el modelo local no puede responder ({}). Por tu privacidad no lo enviaré a internet.",
                privacy.summary(),
                local.why_not.unwrap_or_default()
            ));
        }
        return d;
    }
    let why = if !local.eligible {
        Some(format!("el modelo local {}", local.why_not.clone().unwrap_or_default()))
    } else if ctx.auto && level == Complexity::Complex {
        Some(format!("la tarea parece compleja ({})", d.reasons.join(", ")))
    } else {
        None
    };
    if let Some(why) = why {
        d.offer = best_cloud.as_ref().map(|c| offer_from(c, why.clone()));
        if !local.eligible && d.offer.is_none() {
            d.blocked = Some(if ctx.local_ok {
                format!("{why} y no hay un modelo externo disponible (falta la clave o se alcanzó el límite de gasto).")
            } else {
                crate::ai::ollama::connect_error_message(&Settings::default())
            });
        }
    }
    d
}

// ------------------------------------------------------------ gasto

/// Reservas de presupuesto de las respuestas en curso: dos respuestas
/// simultáneas no pueden gastar el mismo saldo.
#[derive(Default)]
pub struct Reservations {
    map: std::sync::Mutex<std::collections::HashMap<String, f64>>,
}

impl Reservations {
    pub fn total(&self) -> f64 {
        self.map.lock().unwrap().values().sum()
    }

    /// Reserva `amount` si cabe en el presupuesto (gastado + reservado).
    pub fn reserve(&self, id: &str, amount: f64, spent: f64, budget: f64) -> Result<(), String> {
        let mut m = self.map.lock().unwrap();
        let reserved: f64 = m.values().sum();
        if budget <= 0.0 || spent + reserved + amount > budget + 1e-9 {
            return Err(format!(
                "Esta respuesta (≈ {amount:.3} US$ estimados) superaría tu límite mensual: gastado {spent:.2} US$ + reservado por otras respuestas en curso {reserved:.2} US$ de {budget:.2} US$."
            ));
        }
        m.insert(id.to_string(), amount);
        Ok(())
    }

    pub fn release(&self, id: &str) -> Option<f64> {
        self.map.lock().unwrap().remove(id)
    }
}

pub static RESERVATIONS: std::sync::LazyLock<Reservations> = std::sync::LazyLock::new(Reservations::default);

/// ¿Se puede usar este proveedor externo ahora? (control final en el backend:
/// presupuesto + privacidad de todo lo que se enviaría + autorizaciones).
pub fn check_cloud_use(
    provider: ProviderId,
    outgoing: &crate::privacy::Outgoing,
    acks: &[String],
    spent_month: f64,
    budget: f64,
) -> Result<(), String> {
    if provider == ProviderId::Ollama {
        return Ok(());
    }
    if budget <= 0.0 || spent_month >= budget {
        return Err(format!(
            "Alcanzaste tu límite de gasto mensual ({spent_month:.2} US$ de {budget:.2} US$). Súbelo en Ajustes › Asistente o usa el modelo local."
        ));
    }
    crate::privacy::check_external(&crate::privacy::scan(outgoing), acks, provider_label(provider))
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
        "api",
    );
    Some(c)
}

/// Estado de Ollama en caché (20 s) para no retrasar cada mensaje.
#[derive(Clone, Copy, Default)]
struct LocalInfo {
    ok: bool,
    vision: bool,
    tools: bool,
}

static LOCAL_CACHE: std::sync::Mutex<Option<(std::time::Instant, String, LocalInfo)>> = std::sync::Mutex::new(None);

async fn local_status(http: &reqwest::Client, settings: &Settings) -> LocalInfo {
    let key = format!("{}|{}", settings.ollama_url, settings.ollama_model);
    if let Some((t, k, info)) = LOCAL_CACHE.lock().unwrap().clone() {
        if k == key && t.elapsed() < std::time::Duration::from_secs(20) {
            return info;
        }
    }
    let st = crate::ai::ollama::status(http, settings).await;
    let has = |c: &str| st.capabilities.iter().any(|x| x == c);
    let info = LocalInfo {
        ok: st.running && st.model_installed,
        vision: has("vision") || crate::ai::model_supports_images(ProviderId::Ollama, &settings.ollama_model) == Some(true),
        // Si Ollama no informa capacidades, se asume que sí (qwen3 las tiene).
        tools: st.capabilities.is_empty() || has("tools"),
    };
    *LOCAL_CACHE.lock().unwrap() = Some((std::time::Instant::now(), key, info));
    info
}

fn has_key(p: ProviderId) -> bool {
    #[cfg(debug_assertions)]
    if std::env::var("CANELA_DEV_API_KEY").is_ok() {
        return true;
    }
    crate::secrets::has_key(p)
}

/// Modelos externos con clave guardada, en orden de preferencia.
fn clouds(settings: &Settings) -> Vec<Model> {
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
    [first, second]
        .into_iter()
        .filter(|p| has_key(*p))
        .map(|p| Model {
            provider: p,
            model: model(p),
        })
        .collect()
}

/// Qué modelo usar para este mensaje (no envía nada a ninguna IA).
#[tauri::command]
pub async fn route_decide(state: tauri::State<'_, crate::commands::AppState>, request: Request) -> Result<Decision, String> {
    let settings = state.current_settings();
    let info = if settings.provider == ProviderId::Ollama || settings.router_auto {
        local_status(&state.http, &settings).await
    } else {
        LocalInfo::default()
    };
    let local_loaded = crate::resources::ollama_ps(&state.http, &crate::ai::ollama::base_url(&settings))
        .await
        .is_some_and(|ps| ps.iter().any(|m| crate::ai::ollama::same_model(&m.name, &settings.ollama_model)));
    let s2 = settings.clone();
    let clouds = tauri::async_runtime::spawn_blocking(move || clouds(&s2))
        .await
        .map_err(|e| e.to_string())?;
    let ctx = Context {
        main: settings.provider,
        auto: settings.router_auto,
        local_ok: info.ok,
        local_vision: info.vision,
        local_tools: info.tools,
        local_ctx_tokens: crate::resources::mode(&settings).max_ctx.min(settings.ollama_num_ctx),
        local_loaded,
        clouds,
        spent_month_usd: state.db.spend_month(&month_key()).unwrap_or(0.0),
        reserved_usd: RESERVATIONS.total(),
        budget_usd: settings.monthly_budget_usd,
    };
    Ok(decide(&request, &ctx))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendSummary {
    pub month: String,
    pub spent_usd: f64,
    /// Parte estimada (respuestas cortadas sin datos reales de consumo).
    pub estimated_usd: f64,
    pub budget_usd: f64,
    pub breakdown: Vec<crate::db::SpendRow>,
}

#[tauri::command]
pub fn spend_summary(state: tauri::State<'_, crate::commands::AppState>) -> Result<SpendSummary, String> {
    let month = month_key();
    Ok(SpendSummary {
        spent_usd: state.db.spend_month(&month)?,
        estimated_usd: state.db.spend_month_estimated(&month)?,
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
            local_tools: true,
            local_ctx_tokens: 16_384,
            local_loaded: true,
            clouds: vec![Model {
                provider: ProviderId::Anthropic,
                model: "claude-opus-5-5".into(),
            }],
            spent_month_usd: 0.0,
            reserved_usd: 0.0,
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
        assert!(private_reason("mi CURP es LOMP900215MDFPRL08").is_some());
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
        c.clouds = vec![];
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
        use crate::privacy::Outgoing;
        let out = |t: &str| Outgoing {
            text: t.into(),
            ..Default::default()
        };
        assert!(check_cloud_use(ProviderId::Ollama, &out("mi contraseña es x"), &[], 99.0, 0.0).is_ok());
        assert!(check_cloud_use(ProviderId::Anthropic, &out("hola"), &[], 1.0, 5.0).is_ok());
        assert!(check_cloud_use(ProviderId::Anthropic, &out("hola"), &[], 5.0, 5.0)
            .unwrap_err()
            .contains("límite"));
        assert!(check_cloud_use(ProviderId::Openai, &out("mi contraseña es x"), &[], 0.0, 5.0)
            .unwrap_err()
            .contains("privacidad"));
        // Datos personales: sólo con la autorización de ese tipo de dato.
        let p = out("escríbele a ana@ejemplo.com");
        assert!(check_cloud_use(ProviderId::Openai, &p, &[], 0.0, 5.0)
            .unwrap_err()
            .contains("autorización"));
        assert!(check_cloud_use(ProviderId::Openai, &p, &["correo".into()], 0.0, 5.0).is_ok());
        // El historial también cuenta.
        let h = Outgoing {
            text: "¿y ahora?".into(),
            history: vec!["DB_PASSWORD=Gato123".into()],
            ..Default::default()
        };
        assert!(check_cloud_use(ProviderId::Openai, &h, &[], 0.0, 5.0).is_err());
    }

    #[test]
    fn personal_data_offer_requires_explicit_ack() {
        let mut r = req(LABELED[23].0);
        r.text.push_str(" y mándale el resultado a jefe@empresa.com");
        let d = decide(&r, &ctx());
        let o = d.offer.expect("propone Claude");
        assert_eq!(o.ack, vec!["correo".to_string()]);
        assert!(o.ack_summary.contains("correo"));
        // Imágenes (no se pueden revisar por dentro) también piden autorización.
        let mut r = req("¿qué ves aquí?");
        r.has_images = true;
        let o = decide(&r, &ctx()).offer.unwrap();
        assert!(o.ack.contains(&"imagen".to_string()));
    }

    #[test]
    fn main_cloud_provider_respects_privacy() {
        let mut c = ctx();
        c.main = ProviderId::Anthropic;
        // Secreto: esta vez responde el modelo local.
        let d = decide(&req("revisa esto: DB_PASSWORD=Gato123"), &c);
        assert_eq!(d.provider, ProviderId::Ollama);
        assert!(d.blocked.is_none());
        // …y si Ollama no está, se bloquea en vez de enviarlo.
        c.local_ok = false;
        assert!(decide(&req("revisa esto: DB_PASSWORD=Gato123"), &c)
            .blocked
            .unwrap()
            .contains("privacidad"));
        // Datos personales: Claude con autorización explícita.
        let d = decide(&req("escribe a ana@ejemplo.com"), &c);
        assert_eq!(d.provider, ProviderId::Anthropic);
        assert_eq!(d.ack_required, vec!["correo".to_string()]);
    }

    #[test]
    fn capability_context_and_speed_drive_the_choice() {
        // Conversación que no cabe en el contexto local → propone el externo.
        let mut r = req("¿y qué opinas de todo esto?");
        r.history_chars = 60_000;
        let d = decide(&r, &ctx());
        assert!(d.offer.unwrap().why.contains("no cabe"), "{:?}", d.candidates[0]);
        // Modelo local sin herramientas + acción → propone el externo.
        let mut c = ctx();
        c.local_tools = false;
        assert!(decide(&req("abre la calculadora"), &c).offer.unwrap().why.contains("herramientas"));
        // Velocidad estimada: cargar el modelo local suma tiempo.
        let mut cold = ctx();
        cold.local_loaded = false;
        let warm_s = decide(&req("hola"), &ctx()).candidates[0].est_seconds;
        let cold_s = decide(&req("hola"), &cold).candidates[0].est_seconds;
        assert!(cold_s > warm_s + 5.0);
        // Todos los candidatos quedan explicados.
        let d = decide(&req(LABELED[24].0), &ctx());
        assert_eq!(d.candidates.len(), 2);
        assert!(d.candidates.iter().all(|c| c.eligible || c.why_not.is_some()));
    }

    #[test]
    fn reservations_prevent_overspending_with_simultaneous_requests() {
        let r = Reservations::default();
        // Presupuesto 1 US$, gastado 0,40: caben dos respuestas de 0,25 pero no tres.
        assert!(r.reserve("a", 0.25, 0.40, 1.0).is_ok());
        assert!(r.reserve("b", 0.25, 0.40, 1.0).is_ok());
        let e = r.reserve("c", 0.25, 0.40, 1.0).unwrap_err();
        assert!(e.contains("reservado por otras respuestas"), "{e}");
        assert!((r.total() - 0.5).abs() < 1e-9);
        r.release("a");
        assert!(r.reserve("c", 0.25, 0.40, 1.0).is_ok(), "al terminar una, se libera su reserva");
        // El enrutador tiene en cuenta lo reservado.
        let mut c = ctx();
        c.spent_month_usd = 3.0;
        c.reserved_usd = 1.99;
        assert!(decide(&req(LABELED[24].0), &c).offer.is_none());
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
