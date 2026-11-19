//! Diagnóstico de memoria: registra RAM, memoria comprometida, VRAM y los
//! procesos que más memoria privada usan, y detecta crecimientos anormales.
//!
//! Conceptos (no son lo mismo):
//! - **RAM física disponible**: lo que queda libre de la RAM real.
//! - **Memoria comprometida** (commit): memoria que Windows prometió a los
//!   programas, respaldada por RAM + archivo de paginación. Puede superar la
//!   RAM física; si llega al límite, los programas fallan. En Windows (WDDM)
//!   las reservas de la GPU también cuentan aquí.
//! - **Memoria privada** de un proceso: lo que sólo ese proceso ha reservado
//!   (cuenta para el commit), esté en RAM o en el archivo de paginación.
//! - **Conjunto de trabajo** (working set / RSS): la parte del proceso que
//!   está ahora mismo en la RAM física.
//! - **VRAM**: memoria de la tarjeta gráfica.
//!
//! Sólo lee datos; nunca cierra procesos ni cambia el archivo de paginación.
//! Todo queda en este equipo (no se envía a ninguna IA).

use serde::Serialize;
use std::collections::{HashMap, VecDeque};

pub const GIB: u64 = 1024 * 1024 * 1024;
pub const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SysMem {
    pub total: u64,
    pub available: u64,
    pub commit_used: u64,
    pub commit_limit: u64,
    pub commit_peak: Option<u64>,
    /// Caché de archivos del sistema (se libera sola si hace falta).
    pub cache: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcMem {
    pub pid: u32,
    pub name: String,
    /// Ruta del ejecutable si Windows permite consultarla.
    pub path: Option<String>,
    /// Memoria privada (Windows: PrivateUsage). En Linux, la residente.
    pub private: u64,
    /// Conjunto de trabajo / residente.
    pub working_set: u64,
    #[serde(skip)]
    pub parent: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuMem {
    pub name: String,
    pub used: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiModelMem {
    pub name: String,
    pub size: u64,
    pub size_vram: u64,
    pub context_length: Option<u64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub ts: String,
    /// Segundos desde que arrancó el registro.
    pub t: u64,
    pub sys: SysMem,
    pub gpu: Option<GpuMem>,
    /// Modelos cargados en Ollama (`/api/ps`). `None` = Ollama no responde.
    pub ai: Option<Vec<AiModelMem>>,
    /// Memoria privada de Canela (proceso principal + sus ventanas web).
    pub canela_private: u64,
    pub canela_working_set: u64,
    /// Los procesos con más memoria privada.
    pub top: Vec<ProcMem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemEvent {
    pub ts: String,
    /// aparece | crece | desaparece | ram_baja | ram_ok | commit_alto | commit_crece_sin_ia | ia_descargada | ia_liberacion
    pub kind: String,
    pub message: String,
    pub pid: Option<u32>,
    pub name: Option<String>,
    pub bytes: Option<u64>,
}

/// «7,5 GB» (coma decimal, como en español).
pub fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / GIB as f64).replace('.', ",")
}

// ------------------------------------------------------------ detector

/// Umbrales del detector.
pub const BIG_PROCESS: u64 = GIB;
pub const FAST_GROWTH: u64 = 512 * MIB;
pub const STEP_GROWTH: u64 = GIB;
pub const COMMIT_GROWTH_NO_AI: u64 = 3 * GIB;
const COMMIT_WINDOW_SECS: u64 = 600;
const LOW_RAM_REPEAT_SECS: u64 = 600;

struct Tracked {
    name: String,
    first_private: u64,
    last_private: u64,
    /// Último múltiplo de STEP_GROWTH ya avisado.
    reported_steps: u64,
    history: VecDeque<(u64, u64)>,
}

#[derive(Default)]
pub struct Detector {
    procs: HashMap<u32, Tracked>,
    low_ram_since: Option<u64>,
    low_ram_last_warn: Option<u64>,
    commit_high: bool,
    commits: VecDeque<(u64, u64, bool)>,
    last_commit_growth_warn: Option<u64>,
    initialized: bool,
}

/// ¿RAM disponible muy baja? (menos de 1,5 GB o del 10 %)
pub fn ram_is_low(s: &SysMem) -> bool {
    s.total > 0 && (s.available < (3 * GIB) / 2 || s.available * 10 < s.total)
}

impl Detector {
    /// Analiza una muestra (con TODOS los procesos) y devuelve los eventos.
    pub fn observe(&mut self, sample: &Sample, all: &[ProcMem]) -> Vec<MemEvent> {
        let t = sample.t;
        let ev = |kind: &str, message: String, p: Option<(&u32, &str)>, bytes: Option<u64>| MemEvent {
            ts: sample.ts.clone(),
            kind: kind.into(),
            message,
            pid: p.map(|(pid, _)| *pid),
            name: p.map(|(_, n)| n.to_string()),
            bytes,
        };
        let mut out = Vec::new();
        let first = !self.initialized;
        self.initialized = true;

        // Procesos: aparecen, crecen, desaparecen.
        let mut seen = std::collections::HashSet::new();
        for p in all {
            seen.insert(p.pid);
            match self.procs.get_mut(&p.pid) {
                None => {
                    if !first && p.private >= BIG_PROCESS {
                        out.push(ev(
                            "aparece",
                            format!("Apareció «{}» (PID {}) usando {} de memoria privada.", p.name, p.pid, gb(p.private)),
                            Some((&p.pid, &p.name)),
                            Some(p.private),
                        ));
                    }
                    let mut history = VecDeque::new();
                    history.push_back((t, p.private));
                    self.procs.insert(
                        p.pid,
                        Tracked {
                            name: p.name.clone(),
                            first_private: p.private,
                            last_private: p.private,
                            reported_steps: 0,
                            history,
                        },
                    );
                }
                Some(tr) => {
                    let jump = p.private.saturating_sub(tr.last_private);
                    let total = p.private.saturating_sub(tr.first_private);
                    let steps = total / STEP_GROWTH;
                    if jump >= FAST_GROWTH {
                        out.push(ev(
                            "crece",
                            format!(
                                "«{}» (PID {}) creció {} desde la medición anterior: ahora usa {}.",
                                p.name,
                                p.pid,
                                gb(jump),
                                gb(p.private)
                            ),
                            Some((&p.pid, &p.name)),
                            Some(p.private),
                        ));
                        tr.reported_steps = tr.reported_steps.max(steps);
                    } else if steps > tr.reported_steps {
                        tr.reported_steps = steps;
                        out.push(ev(
                            "crece",
                            format!(
                                "«{}» (PID {}) lleva {} más que cuando apareció: ahora usa {}.",
                                p.name,
                                p.pid,
                                gb(total),
                                gb(p.private)
                            ),
                            Some((&p.pid, &p.name)),
                            Some(p.private),
                        ));
                    }
                    tr.last_private = p.private;
                    tr.history.push_back((t, p.private));
                    while tr.history.front().is_some_and(|(ht, _)| t.saturating_sub(*ht) > COMMIT_WINDOW_SECS) {
                        tr.history.pop_front();
                    }
                }
            }
        }
        let gone: Vec<u32> = self.procs.keys().filter(|pid| !seen.contains(pid)).copied().collect();
        for pid in gone {
            if let Some(tr) = self.procs.remove(&pid) {
                if tr.last_private >= BIG_PROCESS {
                    out.push(ev(
                        "desaparece",
                        format!("Terminó «{}» (PID {}), que usaba {}.", tr.name, pid, gb(tr.last_private)),
                        Some((&pid, &tr.name)),
                        Some(tr.last_private),
                    ));
                }
            }
        }

        // RAM baja (con repetición cada 10 min mientras dure).
        let s = &sample.sys;
        if ram_is_low(s) {
            let warn = match self.low_ram_last_warn {
                None => true,
                Some(w) => t.saturating_sub(w) >= LOW_RAM_REPEAT_SECS,
            };
            self.low_ram_since.get_or_insert(t);
            if warn {
                self.low_ram_last_warn = Some(t);
                let tops: Vec<String> = sample
                    .top
                    .iter()
                    .take(3)
                    .map(|p| format!("{} ({})", p.name, gb(p.private)))
                    .collect();
                out.push(ev(
                    "ram_baja",
                    format!(
                        "Queda poca RAM libre: {} de {}. Los que más usan: {}.",
                        gb(s.available),
                        gb(s.total),
                        tops.join(", ")
                    ),
                    None,
                    Some(s.available),
                ));
            }
        } else if self.low_ram_since.take().is_some() && s.available > 5 * GIB / 2 {
            self.low_ram_last_warn = None;
            out.push(ev(
                "ram_ok",
                format!("La RAM libre se recuperó: {}.", gb(s.available)),
                None,
                Some(s.available),
            ));
        } else if self.low_ram_since.is_none() {
            self.low_ram_last_warn = None;
        }

        // Memoria comprometida cerca del límite.
        if s.commit_limit > 0 {
            let high = s.commit_used * 100 >= s.commit_limit * 90;
            if high && !self.commit_high {
                out.push(ev(
                    "commit_alto",
                    format!(
                        "La memoria comprometida está al {} % del límite ({} de {}). Si llega al 100 %, los programas pueden fallar.",
                        s.commit_used * 100 / s.commit_limit,
                        gb(s.commit_used),
                        gb(s.commit_limit)
                    ),
                    None,
                    Some(s.commit_used),
                ));
            }
            self.commit_high = high;
        }

        // ¿Sube la memoria comprometida sin ningún modelo de IA cargado?
        let ai_loaded = sample.ai.as_ref().is_some_and(|m| !m.is_empty());
        self.commits.push_back((t, s.commit_used, ai_loaded));
        while self
            .commits
            .front()
            .is_some_and(|(ct, _, _)| t.saturating_sub(*ct) > COMMIT_WINDOW_SECS)
        {
            self.commits.pop_front();
        }
        if let Some(&(t0, c0, _)) = self.commits.front() {
            let no_ai_all = self.commits.iter().all(|(_, _, a)| !a);
            let growth = s.commit_used.saturating_sub(c0);
            let can_warn = self
                .last_commit_growth_warn
                .is_none_or(|w| t.saturating_sub(w) >= COMMIT_WINDOW_SECS);
            if no_ai_all && growth >= COMMIT_GROWTH_NO_AI && can_warn {
                self.last_commit_growth_warn = Some(t);
                // Quiénes crecieron en la misma ventana de tiempo.
                let mut growers: Vec<(String, u32, u64)> = self
                    .procs
                    .iter()
                    .filter_map(|(pid, tr)| {
                        let start = tr.history.iter().find(|(ht, _)| *ht >= t0).map(|(_, v)| *v)?;
                        let g = tr.last_private.saturating_sub(start);
                        (g >= 256 * MIB).then(|| (tr.name.clone(), *pid, g))
                    })
                    .collect();
                growers.sort_by_key(|g| std::cmp::Reverse(g.2));
                let who = if growers.is_empty() {
                    "ningún proceso visible creció tanto (puede ser memoria del sistema, de controladores o de un proceso que ya terminó)"
                        .to_string()
                } else {
                    growers
                        .iter()
                        .take(4)
                        .map(|(n, pid, g)| format!("{n} (PID {pid}) +{}", gb(*g)))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                out.push(ev(
                    "commit_crece_sin_ia",
                    format!(
                        "La memoria comprometida subió {} en {} min sin ningún modelo de IA cargado. Crecieron: {who}.",
                        gb(growth),
                        (t - t0).div_ceil(60).max(1)
                    ),
                    None,
                    Some(growth),
                ));
            }
        }
        out
    }
}

// ------------------------------------------------------------ lectura

/// Memoria del sistema.
#[cfg(windows)]
pub fn read_sys() -> Option<SysMem> {
    use windows_sys::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
    let mut pi: PERFORMANCE_INFORMATION = unsafe { std::mem::zeroed() };
    pi.cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
    // SAFETY: estructura inicializada con su tamaño.
    if unsafe { GetPerformanceInfo(&mut pi, pi.cb) } == 0 {
        return None;
    }
    let page = pi.PageSize as u64;
    Some(SysMem {
        total: pi.PhysicalTotal as u64 * page,
        available: pi.PhysicalAvailable as u64 * page,
        commit_used: pi.CommitTotal as u64 * page,
        commit_limit: pi.CommitLimit as u64 * page,
        commit_peak: Some(pi.CommitPeak as u64 * page),
        cache: Some(pi.SystemCache as u64 * page),
    })
}

#[cfg(not(windows))]
pub fn read_sys() -> Option<SysMem> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// Linux: `/proc/meminfo` (en kB).
#[cfg_attr(windows, allow(dead_code))]
pub fn parse_meminfo(text: &str) -> Option<SysMem> {
    let get = |key: &str| {
        text.lines()
            .find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    Some(SysMem {
        total: get("MemTotal")?,
        available: get("MemAvailable")?,
        commit_used: get("Committed_AS").unwrap_or(0),
        commit_limit: get("CommitLimit").unwrap_or(0),
        commit_peak: None,
        cache: get("Cached"),
    })
}

/// Lee todos los procesos (memoria privada y física).
pub fn read_processes(sys: &mut sysinfo::System) -> Vec<ProcMem> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory().with_exe(UpdateKind::OnlyIfNotSet),
    );
    sys.processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| {
            let ws = p.memory();
            // Windows: virtual_memory() = PrivateUsage (memoria privada).
            #[cfg(windows)]
            let private = p.virtual_memory();
            #[cfg(not(windows))]
            let private = ws;
            ProcMem {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().to_string(),
                path: p.exe().map(|e| e.display().to_string()),
                private,
                working_set: ws,
                parent: p.parent().map(|pp| pp.as_u32()),
            }
        })
        .collect()
}

/// Memoria de Canela: su proceso y todos sus descendientes (WebView2).
pub fn canela_usage(all: &[ProcMem], own: u32) -> (u64, u64) {
    let mut ids = std::collections::HashSet::from([own]);
    loop {
        let before = ids.len();
        for p in all {
            if p.parent.is_some_and(|pp| ids.contains(&pp)) {
                ids.insert(p.pid);
            }
        }
        if ids.len() == before {
            break;
        }
    }
    all.iter()
        .filter(|p| ids.contains(&p.pid))
        .fold((0, 0), |(a, b), p| (a + p.private, b + p.working_set))
}

pub fn top(all: &[ProcMem], n: usize) -> Vec<ProcMem> {
    let mut v = all.to_vec();
    v.sort_by_key(|p| std::cmp::Reverse(p.private));
    v.truncate(n);
    v
}

/// `nvidia-smi --query-gpu=name,memory.used,memory.total --format=csv,noheader,nounits`
pub fn parse_nvidia_smi(out: &str) -> Option<GpuMem> {
    let line = out.lines().find(|l| !l.trim().is_empty())?;
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return None;
    }
    Some(GpuMem {
        name: parts[0].to_string(),
        used: parts[1].parse::<u64>().ok()? * MIB,
        total: parts[2].parse::<u64>().ok()? * MIB,
    })
}

/// VRAM de la GPU NVIDIA con `nvidia-smi` (comando fijo de solo lectura,
/// sin ventana, con límite de 3 s). `None` si no hay GPU NVIDIA.
pub fn read_gpu() -> Option<GpuMem> {
    let exe = nvidia_smi_path()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["--query-gpu=name,memory.used,memory.total", "--format=csv,noheader,nounits"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let _ = tx.send(cmd.output());
    });
    let out = rx.recv_timeout(std::time::Duration::from_secs(3)).ok()?.ok()?;
    parse_nvidia_smi(&String::from_utf8_lossy(&out.stdout))
}

fn nvidia_smi_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        [
            std::path::Path::new(&sysroot).join(r"System32\nvidia-smi.exe"),
            std::path::Path::new(&pf).join(r"NVIDIA Corporation\NVSMI\nvidia-smi.exe"),
        ]
        .into_iter()
        .find(|p| p.exists())
    }
    #[cfg(not(windows))]
    {
        ["/usr/bin/nvidia-smi", "/usr/local/bin/nvidia-smi"]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.exists())
    }
}

/// Respuesta de `GET /api/ps` de Ollama.
pub fn parse_ollama_ps(v: &serde_json::Value) -> Vec<AiModelMem> {
    v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(AiModelMem {
                name: m["name"].as_str().or(m["model"].as_str())?.to_string(),
                size: m["size"].as_u64().unwrap_or(0),
                size_vram: m["size_vram"].as_u64().unwrap_or(0),
                context_length: m["context_length"].as_u64(),
                expires_at: m["expires_at"].as_str().map(str::to_string),
            })
        })
        .collect()
}

/// Informe en texto (para exportar o leer en Ajustes).
pub fn sample_summary(s: &Sample) -> String {
    let mut out = vec![format!(
        "RAM: {} libres de {} · Memoria comprometida: {} de {}{}",
        gb(s.sys.available),
        gb(s.sys.total),
        gb(s.sys.commit_used),
        gb(s.sys.commit_limit),
        s.sys.commit_peak.map(|p| format!(" (pico {})", gb(p))).unwrap_or_default()
    )];
    if let Some(g) = &s.gpu {
        out.push(format!("GPU {}: {} de {} de VRAM", g.name, gb(g.used), gb(g.total)));
    }
    match &s.ai {
        None => out.push("Ollama: no responde".into()),
        Some(m) if m.is_empty() => out.push("Ollama: ningún modelo cargado".into()),
        Some(m) => {
            for x in m {
                out.push(format!(
                    "Ollama: {} cargado ({} en total, {} en VRAM)",
                    x.name,
                    gb(x.size),
                    gb(x.size_vram)
                ));
            }
        }
    }
    out.push(format!(
        "Canela (con sus ventanas): {} privada, {} en RAM",
        gb(s.canela_private),
        gb(s.canela_working_set)
    ));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(avail_gb: u64, commit_gb: u64) -> SysMem {
        SysMem {
            total: 16 * GIB,
            available: avail_gb * GIB,
            commit_used: commit_gb * GIB,
            commit_limit: 50 * GIB,
            commit_peak: None,
            cache: None,
        }
    }

    fn p(pid: u32, name: &str, private_mb: u64) -> ProcMem {
        ProcMem {
            pid,
            name: name.into(),
            path: None,
            private: private_mb * MIB,
            working_set: private_mb * MIB,
            parent: None,
        }
    }

    fn sample(t: u64, s: SysMem, ai: bool, procs: &[ProcMem]) -> Sample {
        Sample {
            ts: format!("t{t}"),
            t,
            sys: s,
            gpu: None,
            ai: Some(if ai {
                vec![AiModelMem {
                    name: "qwen3:8b".into(),
                    ..Default::default()
                }]
            } else {
                vec![]
            }),
            canela_private: 0,
            canela_working_set: 0,
            top: top(procs, 5),
        }
    }

    #[test]
    fn detects_processes_that_appear_grow_and_vanish() {
        let mut d = Detector::default();
        let base = [p(1, "explorer.exe", 200), p(2, "chrome.exe", 900)];
        assert!(
            d.observe(&sample(0, sys(8, 20), false, &base), &base).is_empty(),
            "la primera muestra sólo sirve de base"
        );
        // Aparece python.exe con 7,3 GB y chrome crece 600 MB de golpe.
        let s2 = [p(1, "explorer.exe", 200), p(2, "chrome.exe", 1500), p(3, "python.exe", 7300)];
        let e = d.observe(&sample(60, sys(5, 26), false, &s2), &s2);
        let kinds: Vec<&str> = e.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"aparece") && kinds.contains(&"crece"), "{e:?}");
        assert!(e.iter().any(|x| x.message.contains("python.exe") && x.message.contains("PID 3")));
        // Desaparece python.exe.
        let s3 = [p(1, "explorer.exe", 200), p(2, "chrome.exe", 1500)];
        let e = d.observe(&sample(120, sys(8, 20), false, &s3), &s3);
        assert_eq!(e[0].kind, "desaparece");
        assert_eq!(e[0].pid, Some(3));
    }

    #[test]
    fn slow_growth_is_reported_per_gigabyte() {
        let mut d = Detector::default();
        let mut events = Vec::new();
        for i in 0..30u64 {
            // +100 MB por minuto: nunca un salto grande, pero 2,9 GB en total.
            let ps = [p(7, "msedgewebview2.exe", 300 + i * 100)];
            events.extend(d.observe(&sample(i * 60, sys(8, 20), false, &ps), &ps));
        }
        let grows: Vec<_> = events.iter().filter(|e| e.kind == "crece").collect();
        assert_eq!(grows.len(), 2, "{grows:?}");
    }

    #[test]
    fn low_ram_warns_then_repeats_and_recovers() {
        let mut d = Detector::default();
        let none: [ProcMem; 0] = [];
        assert!(d.observe(&sample(0, sys(8, 20), false, &none), &none).is_empty());
        let e = d.observe(&sample(60, sys(1, 30), false, &none), &none);
        assert_eq!(e[0].kind, "ram_baja");
        assert!(
            d.observe(&sample(120, sys(1, 30), false, &none), &none).is_empty(),
            "no repite enseguida"
        );
        let e = d.observe(&sample(60 + 600, sys(1, 30), false, &none), &none);
        assert_eq!(e[0].kind, "ram_baja", "repite a los 10 min");
        let e = d.observe(&sample(800, sys(6, 20), false, &none), &none);
        assert_eq!(e[0].kind, "ram_ok");
    }

    #[test]
    fn commit_growth_without_ai_names_the_growers() {
        let mut d = Detector::default();
        let mut found = None;
        for i in 0..11u64 {
            let ps = [p(10, "python.exe", 500 + i * 400), p(11, "explorer.exe", 200)];
            let e = d.observe(&sample(i * 60, sys(8, 20 + i / 2), false, &ps), &ps);
            if let Some(x) = e.into_iter().find(|x| x.kind == "commit_crece_sin_ia") {
                found = Some(x);
                break;
            }
        }
        let x = found.expect("debió detectar el crecimiento");
        assert!(x.message.contains("python.exe (PID 10)"), "{}", x.message);
        // Con un modelo cargado no se considera anormal.
        let mut d = Detector::default();
        for i in 0..11u64 {
            let ps = [p(10, "ollama.exe", 500)];
            let e = d.observe(&sample(i * 60, sys(8, 20 + i), true, &ps), &ps);
            assert!(!e.iter().any(|x| x.kind == "commit_crece_sin_ia"));
        }
    }

    #[test]
    fn commit_near_limit_warns_once() {
        let mut d = Detector::default();
        let none: [ProcMem; 0] = [];
        d.observe(&sample(0, sys(8, 30), true, &none), &none);
        let e = d.observe(&sample(60, sys(8, 46), true, &none), &none);
        assert!(e.iter().any(|x| x.kind == "commit_alto" && x.message.contains("92 %")), "{e:?}");
        assert!(d.observe(&sample(120, sys(8, 47), true, &none), &none).is_empty());
    }

    #[test]
    fn parsers() {
        let g = parse_nvidia_smi("NVIDIA GeForce RTX 5060 Ti, 7712, 16311\n").unwrap();
        assert_eq!(g.name, "NVIDIA GeForce RTX 5060 Ti");
        assert_eq!(g.used, 7712 * MIB);
        assert!(parse_nvidia_smi("").is_none());
        let m = parse_meminfo("MemTotal:       16000000 kB\nMemFree: 1 kB\nMemAvailable:    6000000 kB\nCached: 10 kB\nCommitLimit:    20000000 kB\nCommitted_AS:   12000000 kB\n").unwrap();
        assert_eq!(m.total, 16_000_000 * 1024);
        assert_eq!(m.commit_used, 12_000_000 * 1024);
        let v = serde_json::json!({"models":[{"name":"qwen3:8b","size":8053063680u64,"size_vram":8053063680u64,"context_length":16384,"expires_at":"2026-11-19T10:00:00Z"}]});
        let a = parse_ollama_ps(&v);
        assert_eq!(a[0].context_length, Some(16384));
        assert!(parse_ollama_ps(&serde_json::json!({"models":[]})).is_empty());
    }

    #[test]
    fn canela_includes_its_webviews() {
        let mut all = vec![
            p(100, "canela-ai.exe", 120),
            p(101, "msedgewebview2.exe", 200),
            p(102, "msedgewebview2.exe", 90),
            p(5, "otro.exe", 999),
        ];
        all[1].parent = Some(100);
        all[2].parent = Some(101);
        let (private, _) = canela_usage(&all, 100);
        assert_eq!(private, 410 * MIB);
    }

    #[test]
    fn real_readings_work_here() {
        let s = read_sys().expect("memoria del sistema");
        assert!(s.total > 0 && s.available <= s.total);
        let mut sys = sysinfo::System::new().unwrap();
        let all = read_processes(&mut sys);
        assert!(all.iter().any(|p| p.pid == std::process::id()));
        let (own, _) = canela_usage(&all, std::process::id());
        assert!(own > 0);
    }
}
