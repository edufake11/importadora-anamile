//! Proveedor Ollama (modelos locales) — API nativa `POST /api/chat` con
//! streaming NDJSON (una línea JSON por trozo).
//!
//! Se usa la API nativa y no la compatible con OpenAI porque sólo la nativa
//! permite fijar el tamaño de contexto (`options.num_ctx`; el valor por
//! defecto de Ollama, 4096 tokens, no alcanza para las instrucciones y las
//! herramientas de Canela) y controlar el razonamiento (`think`).
//!
//! Privacidad: sólo se aceptan direcciones de este equipo (127.0.0.1,
//! localhost, ::1); la conversación nunca sale de la computadora.

use super::{ChatMessage, ParseOut, ProviderStream, Role, StreamEvent, ToolCall, ToolDef, ToolResult, TurnOutcome};
use crate::ai::sse::SseEvent;
use crate::settings::{ProviderId, Settings};
use crate::vision::ImageData;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "qwen3:8b";
pub const DEFAULT_NUM_CTX: u32 = 16_384;
pub const MIN_NUM_CTX: u32 = 4_096;
pub const MAX_NUM_CTX: u32 = 131_072;

/// ¿Es una dirección de este mismo equipo? Devuelve la URL base normalizada.
pub fn loopback_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let local = match url.host()? {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    };
    if !local {
        return None;
    }
    let port = url.port_or_known_default().unwrap_or(11434);
    let host = match url.host()? {
        url::Host::Ipv6(ip) => format!("[{ip}]"),
        h => h.to_string(),
    };
    Some(format!("{}://{host}:{port}", url.scheme()))
}

/// URL base efectiva (siempre local).
pub fn base_url(settings: &Settings) -> String {
    loopback_url(&settings.ollama_url).unwrap_or_else(|| DEFAULT_URL.to_string())
}

// ------------------------------------------------------------ compatibilidad

/// Modelos que respondieron que no admiten razonamiento o herramientas: en
/// la siguiente petición se omite ese campo (ver `ai::complete`).
static NO_THINK: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static NO_TOOLS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn in_set(set: &Mutex<Option<HashSet<String>>>, model: &str) -> bool {
    set.lock().unwrap().as_ref().is_some_and(|s| s.contains(model))
}

fn add_to(set: &Mutex<Option<HashSet<String>>>, model: &str) {
    set.lock().unwrap().get_or_insert_with(HashSet::new).insert(model.to_string());
}

pub fn tools_unsupported(model: &str) -> bool {
    in_set(&NO_TOOLS, model)
}

/// Si el error de Ollama dice que el modelo no admite `think` o `tools`, lo
/// recuerda y devuelve `true` para reintentar sin ese campo.
pub fn learn_from_error(model: &str, body: &str) -> bool {
    let b = body.to_lowercase();
    if b.contains("does not support thinking") && !in_set(&NO_THINK, model) {
        add_to(&NO_THINK, model);
        return true;
    }
    if b.contains("does not support tools") && !in_set(&NO_TOOLS, model) {
        add_to(&NO_TOOLS, model);
        return true;
    }
    false
}

// ------------------------------------------------------------ mensajes

fn images_b64(images: &[ImageData]) -> Vec<String> {
    images.iter().map(|i| i.data.clone()).collect()
}

pub fn native_messages(history: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in history {
        match m.role {
            Role::User => {
                let mut msg = json!({ "role": "user", "content": m.text });
                if !m.images.is_empty() {
                    msg["images"] = json!(images_b64(&m.images));
                }
                out.push(msg);
            }
            Role::Assistant => {
                if m.provider == Some(ProviderId::Ollama) {
                    if let Some(t) = m.transcript.as_ref().filter(|t| !t.is_empty()) {
                        out.extend(t.iter().cloned());
                        continue;
                    }
                }
                if !m.text.trim().is_empty() {
                    out.push(json!({ "role": "assistant", "content": m.text }));
                }
            }
        }
    }
    out
}

pub fn tool_results_messages(results: &[ToolResult]) -> Vec<Value> {
    results
        .iter()
        .map(|r| {
            let content = if r.is_error { format!("ERROR: {}", r.text) } else { r.text.clone() };
            let mut m = json!({ "role": "tool", "tool_name": r.name, "content": content });
            if !r.images.is_empty() {
                m["images"] = json!(images_b64(&r.images));
            }
            m
        })
        .collect()
}

pub fn build_body(settings: &Settings, system: &str, messages: &[Value], tools: &[ToolDef]) -> Value {
    let model = settings.ollama_model.trim();
    let params = crate::resources::mode(settings);
    let mut msgs = vec![json!({ "role": "system", "content": system })];
    msgs.extend(messages.iter().cloned());
    let mut body = json!({
        "model": model,
        "messages": msgs,
        "stream": true,
        // Ollama descarga el modelo solo tras este tiempo sin peticiones
        // (2 min en Equilibrado, 30 s en Ahorro de recursos).
        "keep_alive": params.keep_alive,
        "options": { "num_ctx": settings.ollama_num_ctx.clamp(MIN_NUM_CTX, MAX_NUM_CTX).min(params.max_ctx) },
    });
    if !in_set(&NO_THINK, model) {
        body["think"] = json!(settings.ollama_think && params.allow_think);
    }
    if !tools.is_empty() && !in_set(&NO_TOOLS, model) {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(
                    |t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } }),
                )
                .collect(),
        );
    }
    body
}

/// Ollama no necesita clave: no se envía ninguna cabecera de autenticación.
pub fn build_request(
    http: &reqwest::Client,
    settings: &Settings,
    system: &str,
    messages: &[Value],
    tools: &[ToolDef],
) -> reqwest::RequestBuilder {
    http.post(format!("{}/api/chat", base_url(settings)))
        .json(&build_body(settings, system, messages, tools))
}

/// Mensaje de error claro a partir de la respuesta HTTP de Ollama.
pub fn error_message(status: u16, body: &str, model: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect());
    let d = detail.to_lowercase();
    if status == 404 || d.contains("not found") && d.contains("model") {
        return format!(
            "El modelo «{model}» no está instalado en Ollama. Instálalo abriendo una terminal y escribiendo:\n  ollama pull {model}\n(o elige otro modelo instalado en Ajustes)."
        );
    }
    if d.contains("out of memory") || d.contains("cudamalloc") || d.contains("requires more system memory") {
        return format!(
            "A Ollama le faltó memoria para cargar «{model}». Baja el tamaño de contexto en Ajustes, cierra juegos o programas pesados, o usa un modelo más pequeño.\nDetalle: {detail}"
        );
    }
    let base = match status {
        400 => "Ollama rechazó la solicitud (400).".to_string(),
        500..=599 => format!("Ollama tuvo un problema interno ({status}). Prueba de nuevo; si sigue, reinicia Ollama."),
        _ => format!("Ollama respondió con un error HTTP {status}."),
    };
    if detail.trim().is_empty() {
        base
    } else {
        format!("{base}\nDetalle: {detail}")
    }
}

pub fn connect_error_message(settings: &Settings) -> String {
    format!(
        "No encuentro Ollama en {}. Ábrelo desde el menú Inicio (el icono de la llama aparece junto al reloj) y vuelve a intentarlo.",
        base_url(settings)
    )
}

// ------------------------------------------------------------ streaming

/// Quita los bloques `<think>…</think>` que algunos modelos (o versiones
/// antiguas de Ollama) escriben dentro del texto, aunque lleguen partidos
/// entre varios trozos.
#[derive(Default)]
pub struct ThinkFilter {
    inside: bool,
    pending: String,
    pub saw_thinking: bool,
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

impl ThinkFilter {
    /// Devuelve el texto visible de este trozo.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut out = String::new();
        loop {
            let tag = if self.inside { CLOSE } else { OPEN };
            if let Some(pos) = self.pending.find(tag) {
                if !self.inside {
                    out.push_str(&self.pending[..pos]);
                    self.saw_thinking = true;
                }
                self.pending.drain(..pos + tag.len());
                self.inside = !self.inside;
                if !self.inside {
                    // Tras </think> suele venir un salto de línea: no se muestra.
                    let trimmed = self.pending.trim_start().len();
                    let cut = self.pending.len() - trimmed;
                    self.pending.drain(..cut);
                }
                continue;
            }
            // Conserva un posible inicio de etiqueta partido al final.
            let keep = (1..tag.len())
                .rev()
                .find(|&n| {
                    self.pending.len() >= n
                        && self.pending.is_char_boundary(self.pending.len() - n)
                        && tag.starts_with(&self.pending[self.pending.len() - n..])
                })
                .unwrap_or(0);
            let flush = self.pending.len() - keep;
            if !self.inside {
                out.push_str(&self.pending[..flush]);
            }
            self.pending.drain(..flush);
            break;
        }
        out
    }

    /// Texto pendiente al terminar (si una etiqueta quedó a medias, se muestra).
    pub fn finish(&mut self) -> String {
        if self.inside {
            self.pending.clear();
            String::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

static CALL_SEQ: AtomicU64 = AtomicU64::new(1);

pub struct OllamaStream {
    model: String,
    num_ctx: u32,
    started: bool,
    thinking_shown: bool,
    text: String,
    filter: ThinkFilter,
    calls: Vec<(String, String, Value)>,
}

impl OllamaStream {
    pub fn new(model: &str, num_ctx: u32) -> Self {
        Self {
            model: model.to_string(),
            num_ctx,
            started: false,
            thinking_shown: false,
            text: String::new(),
            filter: ThinkFilter::default(),
            calls: Vec::new(),
        }
    }

    fn outcome(&mut self, done_reason: Option<String>, usage: super::pricing::Usage, prompt_tokens: u64) -> TurnOutcome {
        let rest = self.filter.finish();
        self.text.push_str(&rest);
        let text = self.text.trim().to_string();
        let mut assistant = json!({ "role": "assistant", "content": text });
        if !self.calls.is_empty() {
            assistant["tool_calls"] = Value::Array(
                self.calls
                    .iter()
                    .map(|(_, name, args)| json!({ "function": { "name": name, "arguments": args } }))
                    .collect(),
            );
        }
        let tool_calls: Vec<ToolCall> = self
            .calls
            .iter()
            .map(|(id, name, args)| ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: if args.is_object() {
                    Ok(args.clone())
                } else {
                    Err("El modelo envió parámetros inválidos para la herramienta.".into())
                },
            })
            .collect();
        let notice = if done_reason.as_deref() == Some("length") {
            Some("La respuesta se cortó porque llegó al límite de longitud del modelo local.".to_string())
        } else if prompt_tokens > 0 && prompt_tokens as f64 >= self.num_ctx as f64 * 0.9 {
            Some(
                "La conversación casi llena la memoria del modelo local: puede olvidar lo del principio. Empieza una nueva o sube el «tamaño de contexto» en Ajustes."
                    .to_string(),
            )
        } else {
            None
        };
        let has_content = !text.is_empty() || !self.calls.is_empty();
        TurnOutcome {
            assistant: has_content.then_some(assistant),
            tool_calls,
            stop_reason: done_reason.or_else(|| Some(if self.calls.is_empty() { "stop" } else { "tool_calls" }.into())),
            model: Some(self.model.clone()),
            notice,
            usage,
        }
    }
}

impl ProviderStream for OllamaStream {
    fn ndjson(&self) -> bool {
        true
    }

    fn on_event(&mut self, ev: &SseEvent) -> Vec<ParseOut> {
        let mut out = Vec::new();
        let Ok(v) = serde_json::from_str::<Value>(&ev.data) else {
            return out;
        };
        if let Some(err) = v["error"].as_str() {
            out.push(ParseOut::Failed(error_message(
                500,
                &json!({ "error": err }).to_string(),
                &self.model,
            )));
            return out;
        }
        if !self.started {
            self.started = true;
            out.push(ParseOut::Event(StreamEvent::Started { model: self.model.clone() }));
        }
        let msg = &v["message"];
        if msg["thinking"].as_str().is_some_and(|t| !t.is_empty()) && !self.thinking_shown {
            self.thinking_shown = true;
            out.push(ParseOut::Event(StreamEvent::Thinking));
        }
        if let Some(content) = msg["content"].as_str() {
            let visible = self.filter.push(content);
            if self.filter.saw_thinking && !self.thinking_shown {
                self.thinking_shown = true;
                out.push(ParseOut::Event(StreamEvent::Thinking));
            }
            if !visible.is_empty() {
                // No se muestran los saltos de línea iniciales (p. ej. tras </think>).
                let visible = if self.text.is_empty() {
                    visible.trim_start().to_string()
                } else {
                    visible
                };
                if !visible.is_empty() {
                    self.text.push_str(&visible);
                    out.push(ParseOut::Event(StreamEvent::Delta { text: visible }));
                }
            }
        }
        for c in msg["tool_calls"].as_array().into_iter().flatten() {
            let f = &c["function"];
            let Some(name) = f["name"].as_str() else { continue };
            let args = match &f["arguments"] {
                Value::String(s) => serde_json::from_str(s).unwrap_or(Value::String(s.clone())),
                Value::Null => json!({}),
                a => a.clone(),
            };
            let id = c["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("ollama-{}", CALL_SEQ.fetch_add(1, Ordering::Relaxed)));
            self.calls.push((id, name.to_string(), args));
        }
        if v["done"].as_bool() == Some(true) {
            let prompt = v["prompt_eval_count"].as_u64().unwrap_or(0);
            let usage = super::pricing::Usage {
                input: prompt,
                output: v["eval_count"].as_u64().unwrap_or(0),
                ..Default::default()
            };
            let reason = v["done_reason"].as_str().map(str::to_string);
            out.push(ParseOut::Finished(self.outcome(reason, usage, prompt)));
        }
        out
    }

    fn finish(&mut self) -> Option<TurnOutcome> {
        None
    }
}

// ------------------------------------------------------------ estado

#[derive(Debug, Clone, serde::Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModel {
    pub name: String,
    pub size_gb: f64,
    pub parameters: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub url: String,
    pub running: bool,
    pub version: Option<String>,
    pub models: Vec<InstalledModel>,
    pub model: String,
    pub model_installed: bool,
    /// Capacidades informadas por `/api/show` (completion, tools, thinking, vision…).
    pub capabilities: Vec<String>,
    pub context_length: Option<u64>,
    pub message: String,
}

pub fn parse_tags(v: &Value) -> Vec<InstalledModel> {
    v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(InstalledModel {
                name: m["name"].as_str().or(m["model"].as_str())?.to_string(),
                size_gb: (m["size"].as_f64().unwrap_or(0.0) / 1e9 * 10.0).round() / 10.0,
                parameters: m["details"]["parameter_size"].as_str().map(str::to_string),
            })
        })
        .collect()
}

/// `qwen3:8b` == `qwen3:8b`; `qwen3` == `qwen3:latest`.
pub fn same_model(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().to_lowercase();
        if s.contains(':') {
            s
        } else {
            format!("{s}:latest")
        }
    };
    norm(a) == norm(b)
}

/// Comprueba si Ollama responde y si el modelo elegido está instalado.
pub async fn status(http: &reqwest::Client, settings: &Settings) -> OllamaStatus {
    let base = base_url(settings);
    let model = settings.ollama_model.trim().to_string();
    let mut st = OllamaStatus {
        url: base.clone(),
        model: model.clone(),
        ..Default::default()
    };
    let quick = std::time::Duration::from_secs(4);
    match http.get(format!("{base}/api/version")).timeout(quick).send().await {
        Ok(r) if r.status().is_success() => {
            st.running = true;
            st.version = r.json::<Value>().await.ok().and_then(|v| v["version"].as_str().map(str::to_string));
        }
        _ => {
            st.message = connect_error_message(settings);
            return st;
        }
    }
    if let Ok(r) = http.get(format!("{base}/api/tags")).timeout(quick).send().await {
        if let Ok(v) = r.json::<Value>().await {
            st.models = parse_tags(&v);
        }
    }
    st.model_installed = st.models.iter().any(|m| same_model(&m.name, &model));
    if !st.model_installed {
        st.message = format!(
            "Ollama funciona, pero «{model}» no está instalado. En una terminal: ollama pull {model}{}",
            if st.models.is_empty() {
                String::new()
            } else {
                " — o elige uno de tus modelos instalados.".to_string()
            }
        );
        return st;
    }
    if let Ok(r) = http
        .post(format!("{base}/api/show"))
        .json(&json!({ "model": model }))
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
    {
        if let Ok(v) = r.json::<Value>().await {
            st.capabilities = v["capabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect();
            st.context_length = v["model_info"]
                .as_object()
                .and_then(|o| o.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, v)| v.as_u64()));
        }
    }
    let tools = st.capabilities.is_empty() || st.capabilities.iter().any(|c| c == "tools");
    st.message = if tools {
        format!(
            "Ollama {} listo con «{model}». Todo se procesa en este equipo.",
            st.version.clone().unwrap_or_default()
        )
    } else {
        format!("«{model}» no admite herramientas: Canela podrá conversar, pero no abrir apps ni tocar archivos. Para eso usa qwen3:8b.")
    };
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(v: Value) -> SseEvent {
        SseEvent {
            event: None,
            data: v.to_string(),
        }
    }

    fn settings() -> Settings {
        Settings {
            provider: ProviderId::Ollama,
            ..Settings::default()
        }
    }

    #[test]
    fn only_local_addresses_are_accepted() {
        assert_eq!(loopback_url("http://127.0.0.1:11434").as_deref(), Some("http://127.0.0.1:11434"));
        assert_eq!(loopback_url("http://localhost:11434/").as_deref(), Some("http://localhost:11434"));
        assert_eq!(loopback_url("http://[::1]:11434").as_deref(), Some("http://[::1]:11434"));
        assert_eq!(loopback_url("http://127.0.0.1").as_deref(), Some("http://127.0.0.1:80"));
        assert!(loopback_url("http://192.168.1.20:11434").is_none());
        assert!(loopback_url("https://ollama.example.com").is_none());
        assert!(loopback_url("http://127.0.0.1.evil.com:11434").is_none());
        assert!(loopback_url("http://user:pw@127.0.0.1:11434").is_none());
        assert!(loopback_url("file:///etc/passwd").is_none());
        let mut s = settings();
        s.ollama_url = "http://10.0.0.5:11434".into();
        assert_eq!(base_url(&s), DEFAULT_URL);
    }

    #[test]
    fn body_has_context_think_and_tools_without_key() {
        let mut s = settings();
        s.ollama_num_ctx = 999_999;
        let tools = vec![ToolDef {
            name: "crear_tarea",
            description: "d",
            schema: json!({"type":"object","properties":{}}),
        }];
        let b = build_body(&s, "SYS", &[json!({"role":"user","content":"hola"})], &tools);
        assert_eq!(b["model"], DEFAULT_MODEL);
        assert_eq!(b["stream"], true);
        assert_eq!(b["think"], false);
        assert_eq!(b["options"]["num_ctx"], MAX_NUM_CTX);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hola");
        assert_eq!(b["tools"][0]["function"]["name"], "crear_tarea");
        let req = build_request(&reqwest::Client::new(), &s, "s", &[], &[]).build().unwrap();
        assert_eq!(req.url().as_str(), "http://127.0.0.1:11434/api/chat");
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn unsupported_think_and_tools_are_remembered() {
        let m = "modelo-viejo:1b";
        let s = Settings {
            ollama_model: m.into(),
            ..settings()
        };
        let tools = vec![ToolDef {
            name: "x",
            description: "d",
            schema: json!({}),
        }];
        assert!(build_body(&s, "", &[], &tools).get("think").is_some());
        assert!(learn_from_error(m, r#"{"error":"\"modelo-viejo:1b\" does not support thinking"}"#));
        assert!(
            !learn_from_error(m, r#"{"error":"does not support thinking"}"#),
            "sólo se reintenta una vez"
        );
        assert!(build_body(&s, "", &[], &tools).get("think").is_none());
        assert!(learn_from_error(
            m,
            r#"{"error":"registry.ollama.ai/library/x does not support tools"}"#
        ));
        assert!(build_body(&s, "", &[], &tools).get("tools").is_none());
        assert!(tools_unsupported(m));
        assert!(!learn_from_error(m, r#"{"error":"otra cosa"}"#));
    }

    #[test]
    fn think_tags_are_hidden_even_when_split() {
        let mut f = ThinkFilter::default();
        let mut out = String::new();
        for chunk in ["Ho", "<thi", "nk>\nel usuario quiere", " algo</th", "ink>\n\n", "la, ", "¿qué tal?"] {
            out.push_str(&f.push(chunk));
        }
        out.push_str(&f.finish());
        assert_eq!(out, "Hola, ¿qué tal?");
        assert!(f.saw_thinking);
        let mut f = ThinkFilter::default();
        assert_eq!(f.push("a < b y 3<4"), "a < b y 3<4");
        assert_eq!(f.push(" fin <thi"), " fin ", "un posible inicio de etiqueta espera");
        assert_eq!(f.finish(), "<thi", "si nunca se completa, se muestra");
        let mut f = ThinkFilter::default();
        assert_eq!(f.push("ñandú <"), "ñandú ");
        assert_eq!(f.push("3"), "<3");
    }

    #[test]
    fn stream_shows_answer_hides_reasoning_and_collects_tools() {
        let mut p = OllamaStream::new("qwen3:8b", 16384);
        let mut events = Vec::new();
        let mut finished = None;
        let lines = [
            json!({"model":"qwen3:8b","message":{"role":"assistant","content":"","thinking":"Debo crear una tarea"},"done":false}),
            json!({"model":"qwen3:8b","message":{"role":"assistant","content":"","thinking":" con fecha."},"done":false}),
            json!({"model":"qwen3:8b","message":{"role":"assistant","content":"¡Guau! "},"done":false}),
            json!({"model":"qwen3:8b","message":{"role":"assistant","content":"La creo.","tool_calls":[{"function":{"name":"crear_tarea","arguments":{"titulo":"Estudiar"}}}]},"done":false}),
            json!({"model":"qwen3:8b","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":1200,"eval_count":40}),
        ];
        for l in lines {
            for o in p.on_event(&ev(l)) {
                match o {
                    ParseOut::Event(e) => events.push(e),
                    ParseOut::Finished(t) => finished = Some(t),
                    ParseOut::Failed(m) => panic!("{m}"),
                }
            }
        }
        let text: String = events
            .iter()
            .filter_map(|e| {
                if let StreamEvent::Delta { text } = e {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(text, "¡Guau! La creo.");
        assert!(!format!("{events:?}").contains("Debo crear"), "el razonamiento no se muestra");
        assert_eq!(events.iter().filter(|e| matches!(e, StreamEvent::Thinking)).count(), 1);
        let t = finished.unwrap();
        assert_eq!(t.tool_calls.len(), 1);
        assert_eq!(t.tool_calls[0].name, "crear_tarea");
        assert_eq!(t.tool_calls[0].input.as_ref().unwrap()["titulo"], "Estudiar");
        assert_eq!(t.usage.input, 1200);
        assert_eq!(t.usage.output, 40);
        let a = t.assistant.unwrap();
        assert_eq!(a["content"], "¡Guau! La creo.");
        assert!(a.get("thinking").is_none(), "el razonamiento no se reenvía");
        assert_eq!(a["tool_calls"][0]["function"]["arguments"]["titulo"], "Estudiar");
    }

    #[test]
    fn stream_errors_and_full_context_are_reported() {
        let mut p = OllamaStream::new("qwen3:8b", 4096);
        let out = p.on_event(&ev(
            json!({"error":"model requires more system memory (9.1 GiB) than is available (6.0 GiB)"}),
        ));
        assert!(matches!(&out[0], ParseOut::Failed(m) if m.contains("faltó memoria")));
        let mut p = OllamaStream::new("qwen3:8b", 4096);
        let out = p.on_event(&ev(json!({"message":{"role":"assistant","content":"hola"},"done":true,"done_reason":"stop","prompt_eval_count":4000,"eval_count":2})));
        let ParseOut::Finished(t) = out.last().unwrap() else { panic!() };
        assert!(t.notice.as_deref().unwrap().contains("casi llena"));
    }

    #[test]
    fn errors_are_clear() {
        let m = error_message(404, r#"{"error":"model 'qwen3:8b' not found"}"#, "qwen3:8b");
        assert!(m.contains("ollama pull qwen3:8b"), "{m}");
        let m = error_message(500, "boom", "x");
        assert!(m.contains("reinicia Ollama"));
        assert!(connect_error_message(&settings()).contains("127.0.0.1:11434"));
    }

    #[test]
    fn history_and_tool_results_use_ollama_format() {
        let img = ImageData {
            media_type: "image/png".into(),
            data: "QUJD".into(),
            width: 1,
            height: 1,
        };
        let h = vec![
            ChatMessage {
                role: Role::User,
                text: "mira".into(),
                provider: None,
                transcript: None,
                images: vec![img.clone()],
                agent: None,
            },
            ChatMessage {
                role: Role::Assistant,
                text: "visto".into(),
                provider: Some(ProviderId::Anthropic),
                transcript: Some(vec![json!({"role":"assistant","content":[{"type":"thinking"}]})]),
                images: vec![],
                agent: None,
            },
        ];
        let n = native_messages(&h);
        assert_eq!(n[0]["images"][0], "QUJD");
        assert_eq!(
            n[1],
            json!({"role":"assistant","content":"visto"}),
            "transcripciones de otro proveedor no se reenvían"
        );
        let r = tool_results_messages(&[ToolResult {
            id: "ollama-1".into(),
            name: "listar_tareas".into(),
            text: "nada".into(),
            is_error: true,
            images: vec![],
        }]);
        assert_eq!(r[0], json!({"role":"tool","tool_name":"listar_tareas","content":"ERROR: nada"}));
    }

    #[test]
    fn installed_models_are_listed_and_matched() {
        let v = json!({"models":[{"name":"qwen3:8b","size":5225388164u64,"details":{"parameter_size":"8.2B"}},{"model":"llama3.2:latest","size":2019393189u64}]});
        let m = parse_tags(&v);
        assert_eq!(m[0].name, "qwen3:8b");
        assert_eq!(m[0].size_gb, 5.2);
        assert_eq!(m[0].parameters.as_deref(), Some("8.2B"));
        assert!(same_model("llama3.2", &m[1].name));
        assert!(same_model("QWEN3:8b", "qwen3:8b"));
        assert!(!same_model("qwen3", "qwen3:8b"));
    }
}

#[cfg(test)]
mod size_tests {
    /// Las instrucciones + herramientas deben caber holgadamente en el
    /// contexto por defecto de Canela (16 384), no en el de Ollama (4 096).
    #[test]
    fn prompt_and_tools_fit_default_context() {
        let s = crate::settings::Settings::default();
        let tools = crate::tools::definitions();
        let body = super::build_body(&s, &crate::ai::system_prompt("Eduardo", &"x".repeat(1500), true), &[], &tools);
        let chars = body.to_string().chars().count();
        // ~3,2 caracteres por token en español con JSON (estimación prudente).
        let tokens = chars * 10 / 32;
        eprintln!(
            "instrucciones + {} herramientas ≈ {chars} caracteres ≈ {tokens} tokens",
            tools.len()
        );
        assert!(tokens > 4096, "si cupiera en 4096 no haría falta subir num_ctx");
        assert!(
            tokens < super::DEFAULT_NUM_CTX as usize / 2,
            "deja al menos la mitad para la conversación"
        );
    }
}
