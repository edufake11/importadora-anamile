//! Coordinación de varios agentes (v0.7.1, en desarrollo).
//!
//! Para peticiones como «revisa mi proyecto web, identifica errores, organiza
//! las tareas pendientes y prepara un informe», Canela:
//! 1. **Planifica**: divide la petición en partes y asigna cada una al agente
//!    adecuado (reglas explicables, sin llamar a ningún modelo para decidir).
//! 2. **Pide permiso** para el plan completo una vez. Lo delicado dentro de
//!    cada paso (escribir, borrar, mover, capturas) se vuelve a preguntar.
//! 3. **Delega** cada paso a su agente con el MISMO modelo (no se cargan
//!    modelos adicionales) y sólo con sus herramientas.
//! 4. **Comparte sólo lo necesario**: cada paso recibe el objetivo, su parte
//!    y los RESÚMENES de los pasos anteriores; nunca los datos internos de las
//!    herramientas de otro paso (contenido de archivos, etc.).
//! 5. **Comprueba** los resultados (verificación de v0.7) y **se recupera**
//!    de errores temporales con un reintento, salvo que el intento fallido ya
//!    hubiera hecho cambios (no se repiten acciones).
//! 6. **Informa**: hechas, pendientes y errores, también si se cancela.

use crate::agents::{profile, select_for_clause, ActionRecord, AgentId};
use crate::ai::agent::{self, AgentEnv, Emit, CONFIRM_TIMEOUT};
use crate::ai::{pricing::Usage, ChatMessage, Role, StreamEvent};
use serde::Serialize;
use std::sync::{Arc, Mutex};

pub const MAX_STEPS: usize = 6;
/// Resumen máximo que pasa de un paso a los siguientes.
pub const MAX_SUMMARY: usize = 1500;
const MAX_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub agent: AgentId,
    pub task: String,
    /// Paso final de redacción (informe/resumen): sin herramientas.
    pub synthesis: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<Step>,
}

/// Estado de un paso, tal como lo ve la interfaz.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    pub index: usize,
    pub agent: AgentId,
    pub name: String,
    pub task: String,
    /// «pendiente» | «en_curso» | «hecho» | «incompleto» | «error» | «cancelado»
    pub status: String,
    pub attempts: u32,
    pub actions: Vec<ActionRecord>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

// ------------------------------------------------------------ planificador

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            c => c,
        })
        .collect()
}

/// Verbos en imperativo con los que suele empezar cada parte de un encargo.
const VERBS: &[&str] = &[
    "revisa",
    "identifica",
    "organiza",
    "ordena",
    "prepara",
    "crea",
    "busca",
    "lee",
    "escribe",
    "guarda",
    "analiza",
    "investiga",
    "resume",
    "haz",
    "genera",
    "comprueba",
    "verifica",
    "diagnostica",
    "mira",
    "abre",
    "apunta",
    "anota",
    "anade",
    "agrega",
    "elimina",
    "borra",
    "compara",
    "redacta",
    "documenta",
    "prueba",
    "corrige",
    "arregla",
    "explica",
    "dime",
    "muestra",
    "lista",
    "recuerdame",
    "averigua",
    "consulta",
    "calcula",
    "planifica",
    "pon",
    "sube",
    "baja",
    "pausa",
    "limpia",
    "mueve",
    "renombra",
    "copia",
    "agenda",
    "actualiza",
    "marca",
    "detecta",
    "encuentra",
    "evalua",
    "mide",
    "reporta",
    "ejecuta",
    "compila",
    "detalla",
    "enumera",
    "propon",
    "sugiere",
    "programa",
];

/// ¿La palabra es un verbo de encargo? (imperativo, «que revises», «revisar»).
fn is_verb(word: &str) -> bool {
    let w = fold(word);
    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
    if w.is_empty() {
        return false;
    }
    VERBS.iter().any(|v| {
        if *v == w {
            return true;
        }
        let (stem, last) = v.split_at(v.len() - 1);
        let forms: Vec<String> = match last {
            "a" => {
                // Subjuntivo con cambios ortográficos: busca→busques, investiga→investigues, organiza→organices.
                let subj = if let Some(s) = stem.strip_suffix('c') {
                    format!("{s}qu")
                } else if let Some(s) = stem.strip_suffix('g') {
                    format!("{s}gu")
                } else if let Some(s) = stem.strip_suffix('z') {
                    format!("{s}c")
                } else {
                    stem.to_string()
                };
                vec![format!("{subj}es"), format!("{stem}ar")]
            }
            "e" => vec![format!("{stem}as"), format!("{stem}er"), format!("{stem}ir")],
            _ => vec![],
        };
        forms.iter().any(|f| f == w)
    })
}

/// Palabras que unen partes («y luego», «después», «finalmente»…).
const CONNECTORS: &[&str] = &[
    "y",
    "e",
    "luego",
    "despues",
    "finalmente",
    "tambien",
    "ademas",
    "por",
    "ultimo",
    "al",
    "final",
    "entonces",
];

/// Divide un encargo en partes. Sólo corta donde la parte siguiente empieza
/// con un verbo de encargo, para no partir «tareas de hoy, mañana y el lunes».
pub fn split_clauses(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    // Vocativo inicial: «Canela, …».
    let start = usize::from(words.first().is_some_and(|w| fold(w).trim_end_matches([',', ':']) == "canela"));
    let mut clauses: Vec<Vec<&str>> = vec![vec![]];
    let mut i = start;
    while i < words.len() {
        let w = words[i];
        let fw = fold(w);
        let ends_with_punct = w.ends_with([',', ';', '.', '?', '!', ':']);
        let is_connector = CONNECTORS.contains(&fw.as_str());
        if is_connector && !clauses.last().unwrap().is_empty() {
            // ¿Qué viene después de los conectores?
            let mut j = i;
            while j < words.len() && CONNECTORS.contains(&fold(words[j]).trim_end_matches(',')) {
                j += 1;
            }
            if j < words.len() && is_verb(words[j]) {
                clauses.push(vec![]);
                i = j;
                continue;
            }
        }
        clauses.last_mut().unwrap().push(w);
        if ends_with_punct {
            let mut j = i + 1;
            while j < words.len() && CONNECTORS.contains(&fold(words[j]).trim_end_matches(',')) {
                j += 1;
            }
            if j < words.len() && is_verb(words[j]) {
                clauses.push(vec![]);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    clauses
        .into_iter()
        .map(|c| c.join(" ").trim_end_matches([',', ';', '.', ':', ' ']).trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

fn is_report(clause: &str) -> bool {
    let t = fold(clause);
    ["informe", "reporte", "resumen", "resume", "redacta", "conclusiones", "documenta lo"]
        .iter()
        .any(|k| t.contains(k))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Propone un plan si la petición necesita al menos DOS agentes distintos.
/// `None` = la responde un solo agente, como en la v0.7.
pub fn propose(text: &str) -> Option<Plan> {
    let clauses = split_clauses(text);
    if clauses.len() < 2 {
        return None;
    }
    let mut steps: Vec<Step> = Vec::new();
    let mut orphan: Vec<String> = Vec::new();
    for c in &clauses {
        let agent = select_for_clause(c);
        if agent == AgentId::General {
            if is_report(c) {
                steps.push(Step {
                    agent: AgentId::General,
                    task: capitalize(c),
                    synthesis: true,
                });
            } else if let Some(last) = steps.last_mut().filter(|s| !s.synthesis) {
                // Parte sin especialista claro: la hace el agente anterior.
                last.task = format!("{}; {c}", last.task);
            } else {
                orphan.push(c.clone());
            }
            continue;
        }
        let task = if orphan.is_empty() {
            capitalize(c)
        } else {
            capitalize(&format!("{}; {c}", std::mem::take(&mut orphan).join("; ")))
        };
        match steps.last_mut() {
            Some(last) if last.agent == agent && !last.synthesis => last.task = format!("{}; {c}", last.task),
            _ => steps.push(Step {
                agent,
                task,
                synthesis: false,
            }),
        }
    }
    let mut specialists: Vec<AgentId> = steps.iter().filter(|s| !s.synthesis).map(|s| s.agent).collect();
    specialists.sort_by_key(|a| *a as u8);
    specialists.dedup();
    if specialists.len() < 2 || steps.len() > MAX_STEPS {
        return None;
    }
    Some(Plan {
        goal: text.trim().to_string(),
        steps,
    })
}

// ------------------------------------------------------------ informe

fn initial_reports(plan: &Plan) -> Vec<StepReport> {
    plan.steps
        .iter()
        .enumerate()
        .map(|(i, s)| StepReport {
            index: i + 1,
            agent: s.agent,
            name: if s.synthesis {
                "Canela (informe)".into()
            } else {
                profile(s.agent).name.to_string()
            },
            task: s.task.clone(),
            status: "pendiente".into(),
            attempts: 0,
            actions: vec![],
            warnings: vec![],
            error: None,
        })
        .collect()
}

/// Resumen de una línea para el aviso final.
pub fn counts_line(steps: &[StepReport]) -> String {
    let n = |s: &str| steps.iter().filter(|x| x.status == s).count();
    let mut parts = vec![format!("{} de {} pasos hechos", n("hecho"), steps.len())];
    for (k, label) in [
        ("incompleto", "incompletos"),
        ("error", "con error"),
        ("cancelado", "cancelados"),
        ("pendiente", "pendientes"),
    ] {
        if n(k) > 0 {
            parts.push(format!("{} {label}", n(k)));
        }
    }
    format!("Plan: {}.", parts.join(", "))
}

/// Si la tarea se cancela (Detener), el futuro se descarta: este guardián
/// envía igualmente el informe con lo que se llegó a hacer.
struct Finisher {
    steps: Arc<Mutex<Vec<StepReport>>>,
    emit: Emit,
    done: bool,
}

impl Finisher {
    fn update(&self) {
        (self.emit)(StreamEvent::Plan {
            steps: self.steps.lock().unwrap().clone(),
            finished: false,
        });
    }
    fn set(&self, i: usize, f: impl FnOnce(&mut StepReport)) {
        f(&mut self.steps.lock().unwrap()[i]);
        self.update();
    }
    fn finish(&mut self) -> Vec<StepReport> {
        self.done = true;
        let steps = self.steps.lock().unwrap().clone();
        (self.emit)(StreamEvent::Plan {
            steps: steps.clone(),
            finished: true,
        });
        steps
    }
}

impl Drop for Finisher {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let steps = {
            let mut s = self.steps.lock().unwrap();
            for st in s.iter_mut().filter(|st| st.status == "en_curso") {
                st.status = "cancelado".into();
                st.error = Some("Detuviste el plan mientras se hacía este paso.".into());
            }
            s.clone()
        };
        (self.emit)(StreamEvent::Plan { steps, finished: true });
    }
}

// ------------------------------------------------------------ contexto

fn cut(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(max).collect::<String>().trim_end())
    }
}

/// Lo ÚNICO que recibe un paso: objetivo, su parte, texto reciente de la
/// conversación (sin datos de herramientas) y resúmenes de pasos anteriores.
fn step_context(plan: &Plan, i: usize, recent: &[ChatMessage], done: &[(StepReport, String)]) -> ChatMessage {
    let step = &plan.steps[i];
    let n = plan.steps.len();
    let mut text = format!(
        "Objetivo general del usuario: {}\n\nTu parte (paso {} de {n}): {}\n",
        plan.goal,
        i + 1,
        step.task
    );
    if !recent.is_empty() {
        text.push_str("\nContexto reciente de la conversación (sólo texto):\n");
        for m in recent {
            let who = if m.role == Role::User { "Usuario" } else { "Canela" };
            text.push_str(&format!("- {who}: {}\n", cut(&m.text, 600)));
        }
    }
    if !done.is_empty() {
        text.push_str("\nResultados de los pasos anteriores (sólo resúmenes):\n");
        for (r, summary) in done {
            let body = match (r.status.as_str(), &r.error) {
                ("error", Some(e)) => format!("ERROR: {e}"),
                ("pendiente" | "cancelado", _) => "no se hizo".to_string(),
                _ => cut(summary, MAX_SUMMARY),
            };
            text.push_str(&format!("- Paso {} · {} ({}): {body}\n", r.index, r.name, r.status));
        }
    }
    text.push_str(if step.synthesis {
        "\nRedacta lo pedido usando SÓLO los resultados anteriores. Si un paso falló o quedó pendiente, dilo claramente; no inventes resultados."
    } else {
        "\nHaz SOLO tu parte con tus herramientas. No digas que hiciste algo que no hiciste. Termina con un resumen breve (máx. 8 líneas) de lo que hiciste y encontraste: es lo único que verán los siguientes pasos."
    });
    ChatMessage {
        role: Role::User,
        text,
        provider: None,
        transcript: None,
        images: vec![],
        agent: None,
    }
}

/// Errores que pueden resolverse reintentando (red, servidor ocupado).
pub fn transient(msg: &str) -> bool {
    let m = msg.to_lowercase();
    [
        "temporal",
        "problema interno",
        "tardó demasiado",
        "error de red",
        "se cortó",
        "se interrumpió",
        "(429)",
    ]
    .iter()
    .any(|k| m.contains(k))
}

struct DoneInfo {
    stop_reason: Option<String>,
    model: Option<String>,
    usage: Usage,
    cost: Option<f64>,
}

#[derive(Default)]
struct Attempt {
    text: String,
    ok_tools: usize,
    report: Option<(Vec<ActionRecord>, Vec<String>)>,
    done: Option<DoneInfo>,
    error: Option<String>,
}

// ------------------------------------------------------------ ejecución

/// Ejecuta un plan dentro de la misma conversación. Siempre termina con
/// `Plan{finished}` + `Done` (o `Error` si el plan no pudo ni empezar).
pub async fn run(env: AgentEnv, plan: Plan, emit: Emit) {
    let _busy = crate::resources::busy();
    let provider = env.settings.provider;
    let mut finisher = Finisher {
        steps: Arc::new(Mutex::new(initial_reports(&plan))),
        emit: emit.clone(),
        done: false,
    };
    let names: Vec<String> = finisher.steps.lock().unwrap().iter().map(|s| s.name.clone()).collect();
    emit(StreamEvent::Agent {
        id: AgentId::General,
        name: "coordinando".into(),
        reason: format!("la petición necesita varios agentes: {}", names.join(", ")),
    });
    finisher.update();

    // 1) Permiso para el plan completo.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let confirm_id = format!(
        "plan-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let rx = env.confirms.ask(&confirm_id);
    let list: Vec<String> = finisher
        .steps
        .lock()
        .unwrap()
        .iter()
        .map(|s| format!("{}. {} — {}", s.index, s.name, s.task))
        .collect();
    emit(StreamEvent::ConfirmRequest {
        confirm_id,
        title: format!("Plan de trabajo con {} pasos", plan.steps.len()),
        details: format!(
            "Para hacerlo, repartiré el trabajo así (mismo modelo, una sola conversación):\n{}\n\nCada paso sólo usa las herramientas de su agente y sólo ve los resúmenes de los pasos anteriores. Lo delicado (escribir, borrar, mover, capturas) te lo volveré a preguntar. Puedes detenerlo con Detener.",
            list.join("\n")
        ),
        diff: None,
        image: None,
    });
    let approved = matches!(tokio::time::timeout(CONFIRM_TIMEOUT, rx).await, Ok(Ok(true)));
    if !approved {
        emit(StreamEvent::Delta {
            text: "No empecé el plan porque no lo autorizaste. Si quieres, dime qué parte hago o pídemelo paso a paso.".into(),
        });
        let steps = finisher.finish();
        emit(StreamEvent::Done {
            stop_reason: Some("plan_denied".into()),
            model: None,
            notice: Some(counts_line(&steps)),
            provider,
            transcript: vec![],
            usage: Usage::default(),
            cost_usd: if provider.is_cloud() { None } else { Some(0.0) },
        });
        return;
    }

    // 2) Pasos.
    let recent: Vec<ChatMessage> = {
        let h = &env.history;
        let before = &h[..h.len().saturating_sub(1)];
        before[before.len().saturating_sub(4)..].to_vec()
    };
    let mut usage = Usage::default();
    let mut cost: Option<f64> = if provider.is_cloud() { None } else { Some(0.0) };
    let mut model: Option<String> = None;
    let mut done: Vec<(StepReport, String)> = Vec::new();
    let mut stop_note: Option<String> = None;
    let n = plan.steps.len();

    for (i, step) in plan.steps.iter().enumerate() {
        let spent = cost.unwrap_or(0.0);
        if env.max_cost_usd.is_some_and(|max| spent >= max) {
            stop_note = Some("Me detuve para no superar tu límite de gasto mensual; los pasos restantes quedaron pendientes.".into());
            break;
        }
        let prof = profile(step.agent);
        finisher.set(i, |s| s.status = "en_curso".into());
        emit(StreamEvent::Delta {
            text: format!(
                "{}**Paso {} de {n} · {}** — {}\n\n",
                if i == 0 { "" } else { "\n\n" },
                i + 1,
                finisher.steps.lock().unwrap()[i].name,
                step.task
            ),
        });

        let mut system = env.system.clone();
        if !prof.prompt.is_empty() && !step.synthesis {
            system.push_str("\n\n");
            system.push_str(prof.prompt);
        }
        system.push_str("\n\nEstás haciendo UN paso de un plan coordinado por Canela; los demás pasos los hacen otros agentes.");
        let context = step_context(&plan, i, &recent, &done);

        let mut final_attempt = Attempt::default();
        let mut attempts = 0;
        while attempts < MAX_ATTEMPTS {
            attempts += 1;
            let state = Arc::new(Mutex::new(Attempt::default()));
            let sink = state.clone();
            let out = emit.clone();
            let sub_emit: Emit = Arc::new(move |e| match e {
                StreamEvent::Delta { ref text } => {
                    sink.lock().unwrap().text.push_str(text);
                    out(e)
                }
                StreamEvent::ToolEnd { ok, .. } => {
                    if ok {
                        sink.lock().unwrap().ok_tools += 1;
                    }
                    out(e)
                }
                StreamEvent::Report { actions, warnings, .. } => sink.lock().unwrap().report = Some((actions, warnings)),
                StreamEvent::Done {
                    stop_reason,
                    model,
                    usage,
                    cost_usd,
                    ..
                } => {
                    sink.lock().unwrap().done = Some(DoneInfo {
                        stop_reason,
                        model,
                        usage,
                        cost: cost_usd,
                    })
                }
                StreamEvent::Error { message } => sink.lock().unwrap().error = Some(message),
                // El coordinador ya informa del agente y del plan.
                StreamEvent::Agent { .. } | StreamEvent::Plan { .. } | StreamEvent::Cancelled => {}
                other => out(other),
            });
            let sub = AgentEnv {
                http: env.http.clone(),
                settings: env.settings.clone(),
                api_key: env.api_key.clone(),
                system: system.clone(),
                history: vec![context.clone()],
                tools: if step.synthesis { None } else { env.tools.clone() },
                confirms: env.confirms.clone(),
                max_cost_usd: env.max_cost_usd.map(|m| (m - cost.unwrap_or(0.0)).max(0.0)),
                agent: Some(prof),
            };
            agent::run(sub, sub_emit).await;
            let a = std::mem::take(&mut *state.lock().unwrap());
            // Lo que gastó también este intento.
            if let Some(d) = &a.done {
                usage.add(&d.usage);
                model = d.model.clone().or(model.take());
                if let Some(c) = d.cost {
                    cost = Some(cost.unwrap_or(0.0) + c);
                }
            }
            let retry = a.error.as_deref().is_some_and(transient) && a.ok_tools == 0 && attempts < MAX_ATTEMPTS;
            final_attempt = a;
            if !retry {
                break;
            }
            emit(StreamEvent::Delta {
                text: "_(error temporal; lo intento otra vez)_\n\n".into(),
            });
            #[cfg(not(test))]
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let a = final_attempt;
        let (actions, warnings) = a.report.clone().unwrap_or_default();
        let incomplete = actions.iter().any(|x| x.status != "ok" || x.verified == Some(false)) || !warnings.is_empty();
        let budget_stop = a.done.as_ref().is_some_and(|d| d.stop_reason.as_deref() == Some("budget"));
        finisher.set(i, |s| {
            s.attempts = attempts;
            s.actions = actions.clone();
            s.warnings = warnings.clone();
            if let Some(err) = &a.error {
                s.status = "error".into();
                s.error = Some(if a.ok_tools > 0 {
                    format!(
                        "{err} (ya se habían hecho {} acciones; no reintenté para no repetirlas)",
                        a.ok_tools
                    )
                } else if attempts > 1 {
                    format!("{err} (falló también al reintentar)")
                } else {
                    err.clone()
                });
            } else if budget_stop {
                s.status = "incompleto".into();
                s.error = Some("Se detuvo por el límite de gasto.".into());
            } else if incomplete {
                s.status = "incompleto".into();
            } else {
                s.status = "hecho".into();
            }
        });
        if let Some(err) = &a.error {
            emit(StreamEvent::Delta {
                text: format!("⚠ Este paso no se pudo completar: {err}"),
            });
        }
        let report = finisher.steps.lock().unwrap()[i].clone();
        done.push((report, a.text));
        if budget_stop {
            stop_note = Some("Me detuve para no superar tu límite de gasto mensual; los pasos restantes quedaron pendientes.".into());
            break;
        }
    }

    // 3) Informe final.
    let steps = finisher.finish();
    let actions: Vec<ActionRecord> = steps.iter().flat_map(|s| s.actions.clone()).collect();
    let mut warnings: Vec<String> = steps.iter().flat_map(|s| s.warnings.clone()).collect();
    warnings.extend(
        steps
            .iter()
            .filter_map(|s| s.error.as_ref().map(|e| format!("Paso {}: {e}", s.index))),
    );
    emit(StreamEvent::Report {
        agent: "coordinando".into(),
        actions,
        warnings,
    });
    let mut notice = counts_line(&steps);
    if let Some(n) = stop_note {
        notice = format!("{notice} {n}");
    }
    emit(StreamEvent::Done {
        stop_reason: Some("plan".into()),
        model,
        notice: Some(notice),
        provider,
        transcript: vec![],
        usage,
        cost_usd: cost,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agents(p: &Plan) -> Vec<(AgentId, bool)> {
        p.steps.iter().map(|s| (s.agent, s.synthesis)).collect()
    }

    #[test]
    fn the_users_example_becomes_a_three_step_plan() {
        let p = propose("Canela, revisa mi proyecto web, identifica errores, organiza las tareas pendientes y prepara un informe").unwrap();
        assert_eq!(
            agents(&p),
            vec![
                (AgentId::Programador, false),
                (AgentId::Productividad, false),
                (AgentId::General, true)
            ]
        );
        assert_eq!(p.steps[0].task, "Revisa mi proyecto web; identifica errores");
        assert_eq!(p.steps[1].task, "Organiza las tareas pendientes");
        assert_eq!(p.steps[2].task, "Prepara un informe");
    }

    #[test]
    fn clauses_are_split_only_before_a_new_order() {
        assert_eq!(
            split_clauses("crea tareas para hoy, mañana y el lunes"),
            vec!["crea tareas para hoy, mañana y el lunes"]
        );
        assert_eq!(
            split_clauses("quiero que revises el código y luego busques el PDF del contrato"),
            vec!["quiero que revises el código", "busques el PDF del contrato"]
        );
        assert_eq!(
            split_clauses("investiga la historia del teckel. Después guarda un resumen en un archivo"),
            vec!["investiga la historia del teckel", "guarda un resumen en un archivo"]
        );
    }

    #[test]
    fn single_agent_requests_do_not_start_a_plan() {
        for t in [
            "crea una tarea para pagar la luz",
            "revisa mi proyecto y corrige los errores de compilación",
            "investiga quién fue Marie Curie y prepara un resumen",
            "hola, ¿cómo estás?",
            "organiza mis descargas por tipo",
        ] {
            assert!(propose(t).is_none(), "{t}");
        }
    }

    /// Conjunto de encargos de varias partes, escrito para evaluar el
    /// planificador (los agentes esperados se etiquetaron a mano).
    #[test]
    fn planner_on_multi_part_requests() {
        use AgentId::*;
        let cases: &[(&str, &[AgentId])] = &[
            (
                "revisa el código de mi API y crea una tarea por cada error",
                &[Programador, Productividad],
            ),
            (
                "investiga qué es Tauri y luego crea una carpeta para el proyecto en el escritorio",
                &[Investigador, Archivos],
            ),
            (
                "¿por qué va lenta mi PC? mira la memoria y apunta una tarea para limpiar el disco",
                &[Sistema, Productividad],
            ),
            (
                "busca el PDF del contrato, léelo y crea una tarea con la fecha de renovación",
                &[Archivos, Productividad],
            ),
            (
                "revisa mi proyecto de Python, busca información sobre el error en Wikipedia y prepara un informe",
                &[Programador, Investigador, General],
            ),
            (
                "diagnostica el rendimiento del equipo y guarda un informe en un archivo del escritorio",
                &[Sistema, Archivos],
            ),
            (
                "organiza mis descargas y después crea una tarea para revisar las facturas",
                &[Archivos, Productividad],
            ),
            (
                "analiza el script de backup, comprueba cuánto espacio libre queda en el disco y redacta conclusiones",
                &[Programador, Sistema, General],
            ),
        ];
        let mut ok = 0;
        for (t, expected) in cases {
            let got: Vec<AgentId> = propose(t).map(|p| p.steps.iter().map(|s| s.agent).collect()).unwrap_or_default();
            if got == *expected {
                ok += 1;
            } else {
                eprintln!("  fallo: {t:?}: esperado {expected:?}, obtenido {got:?}");
            }
        }
        eprintln!("Planificador: {ok}/{}", cases.len());
        assert!(ok * 100 / cases.len() >= 75, "{ok}/{}", cases.len());
    }

    /// Conjunto NUEVO, escrito después de ajustar el planificador y medido
    /// una sola vez. Incluye peticiones de un solo agente (no deben crear plan).
    #[test]
    fn planner_on_held_out_requests() {
        use AgentId::*;
        let cases: &[(&str, &[AgentId])] = &[
            (
                "lee el archivo notas.txt y crea una tarea por cada pendiente que encuentres",
                &[Archivos, Productividad],
            ),
            (
                "busca información sobre el teckel en Wikipedia y guárdala en un archivo de mi escritorio",
                &[Investigador, Archivos],
            ),
            (
                "revisa por qué no compila mi proyecto de Rust y después dime cuánta memoria RAM queda",
                &[Programador, Sistema],
            ),
            (
                "mira qué proceso consume tanta CPU y apunta una tarea para revisarlo mañana",
                &[Sistema, Productividad],
            ),
            (
                "ordena mi carpeta de descargas, revisa los scripts de Python que haya y haz un resumen",
                &[Archivos, Programador, General],
            ),
            (
                "investiga qué es una API REST, explícame el código de mi servidor y redacta un informe",
                &[Investigador, Programador, General],
            ),
            ("crea una tarea para estudiar y luego pon música", &[]),
            ("analiza el rendimiento del equipo, comprueba el disco y prepara un informe", &[]),
            (
                "quiero que busques el PDF de la factura y que agendes una tarea para pagarla",
                &[Archivos, Productividad],
            ),
            (
                "revisa los errores de mi página HTML, crea una carpeta de respaldo y apunta las tareas pendientes",
                &[Programador, Archivos, Productividad],
            ),
            (
                "¿quién fue Ada Lovelace? Después crea una tarea para leer su biografía",
                &[Investigador, Productividad],
            ),
            (
                "diagnostica por qué se congela el equipo y organiza mis descargas",
                &[Sistema, Archivos],
            ),
            ("dime qué tareas tengo pendientes", &[]),
            ("lee el informe de ventas y resúmelo", &[]),
            (
                "refactoriza la función de login, escribe pruebas unitarias y crea una tarea para revisarlas el lunes",
                &[Programador, Productividad],
            ),
        ];
        let mut ok = 0;
        for (t, expected) in cases {
            let got: Vec<AgentId> = propose(t).map(|p| p.steps.iter().map(|s| s.agent).collect()).unwrap_or_default();
            if got == *expected {
                ok += 1;
            } else {
                eprintln!("  fallo: {t:?}: esperado {expected:?}, obtenido {got:?}");
            }
        }
        eprintln!("Planificador (conjunto nuevo): {ok}/{}", cases.len());
        // Umbral de regresión; el resultado real se documenta.
        assert!(ok * 100 / cases.len() >= 60, "{ok}/{}", cases.len());
    }

    #[test]
    fn transient_errors_are_recognized() {
        assert!(transient(
            "Ollama tuvo un problema interno (500). Prueba de nuevo; si sigue, reinicia Ollama."
        ));
        assert!(transient(
            "OpenAI tiene problemas temporales (503). Inténtalo de nuevo en unos segundos."
        ));
        assert!(transient("Se cortó la conexión con Ollama: eof"));
        assert!(!transient(
            "La clave API de OpenAI no es válida o fue revocada. Revísala en Ajustes."
        ));
        assert!(!transient("El modelo «x» no está instalado en Ollama."));
    }

    #[test]
    fn counts_line_is_honest() {
        let p = propose("revisa el código de mi API y crea una tarea por cada error").unwrap();
        let mut r = initial_reports(&p);
        r[0].status = "hecho".into();
        r[1].status = "error".into();
        assert_eq!(counts_line(&r), "Plan: 1 de 2 pasos hechos, 1 con error.");
    }
}
