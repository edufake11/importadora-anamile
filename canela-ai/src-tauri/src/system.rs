//! Información del sistema para preguntas como «¿por qué va lenta mi PC?» o
//! «¿cuánto espacio me queda?».
//!
//! Privacidad: no se incluyen el nombre del equipo, el usuario, rutas de
//! archivos ni líneas de comando; de los programas sólo el nombre.

use sysinfo::{Disks, ProcessesToUpdate, System};

#[derive(Debug, Clone, PartialEq)]
pub struct DiskInfo {
    pub name: String,
    pub total: u64,
    pub free: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub name: String,
    pub memory: u64,
    pub cpu: f32,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub os: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub cpu_usage: f32,
    pub mem_total: u64,
    pub mem_used: u64,
    /// Memoria comprometida (RAM + archivo de paginación) y su límite.
    pub commit_used: u64,
    pub commit_limit: u64,
    pub uptime_secs: u64,
    pub disks: Vec<DiskInfo>,
    pub battery: Option<Battery>,
    pub top: Vec<ProcInfo>,
}

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / GB)
}

fn pct(part: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (part as f64 * 100.0 / total as f64).round() as u64
    }
}

/// Toma una foto del estado del equipo (tarda ~0,3 s por la medición de CPU).
pub fn snapshot(with_processes: bool) -> Result<Snapshot, String> {
    let mut sys = System::new().map_err(|e| format!("No pude leer el estado del equipo: {e}"))?;
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(250)));
    sys.refresh_cpu_usage();
    let mut top = Vec::new();
    if with_processes {
        sys.refresh_processes(ProcessesToUpdate::All, true);
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(250)));
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut by_name: std::collections::HashMap<String, ProcInfo> = std::collections::HashMap::new();
        for p in sys.processes().values() {
            // En Linux los hilos aparecen como procesos: se omiten.
            if p.thread_kind().is_some() {
                continue;
            }
            let name = p.name().to_string_lossy().trim_end_matches(".exe").to_string();
            if name.is_empty() {
                continue;
            }
            let e = by_name.entry(name.to_lowercase()).or_insert(ProcInfo {
                name,
                memory: 0,
                cpu: 0.0,
                count: 0,
            });
            e.memory += p.memory();
            e.cpu += p.cpu_usage();
            e.count += 1;
        }
        top = by_name.into_values().collect();
        top.sort_by_key(|p| std::cmp::Reverse(p.memory));
        top.truncate(6);
    }
    let cores = sys.cpus().len().max(1);
    let disks: Vec<DiskInfo> = Disks::new_with_refreshed_list()
        .map(|list| {
            list.iter()
                .filter(|d| d.total_space() > 1_000_000_000)
                .take(8)
                .enumerate()
                .map(|(i, d)| DiskInfo {
                    name: disk_label(&d.mount_point().to_string_lossy(), i),
                    total: d.total_space(),
                    free: d.available_space(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Snapshot {
        os: System::long_os_version().or_else(|_| System::name()).unwrap_or_default(),
        cpu_brand: sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        cpu_cores: cores,
        cpu_usage: sys.global_cpu_usage(),
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        commit_used: crate::memdiag::read_sys().map(|m| m.commit_used).unwrap_or(0),
        commit_limit: crate::memdiag::read_sys().map(|m| m.commit_limit).unwrap_or(0),
        uptime_secs: System::uptime().unwrap_or(0),
        disks,
        battery: battery(),
        // El uso de CPU por proceso se reparte entre núcleos para que sume ≤100 %.
        top: top
            .into_iter()
            .map(|mut p| {
                p.cpu /= cores as f32;
                p
            })
            .collect(),
    })
}

/// Sólo letras de unidad («C:\\») o la raíz «/»: otros puntos de montaje
/// pueden contener el nombre de usuario.
fn disk_label(mount: &str, i: usize) -> String {
    let b = mount.as_bytes();
    let is_drive = (b.len() == 2 || b.len() == 3) && b[0].is_ascii_alphabetic() && b[1] == b':';
    if is_drive || mount == "/" {
        mount.to_string()
    } else {
        format!("unidad {}", i + 1)
    }
}

#[cfg(windows)]
fn battery() -> Option<Battery> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut st: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: puntero válido a una estructura inicializada.
    if unsafe { GetSystemPowerStatus(&mut st) } == 0 || st.BatteryFlag & 128 != 0 || st.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery {
        percent: st.BatteryLifePercent,
        charging: st.ACLineStatus == 1,
    })
}

#[cfg(not(windows))]
fn battery() -> Option<Battery> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in dir.flatten() {
        if !e.file_name().to_string_lossy().starts_with("BAT") {
            continue;
        }
        let cap = std::fs::read_to_string(e.path().join("capacity")).ok()?;
        let status = std::fs::read_to_string(e.path().join("status")).unwrap_or_default();
        return Some(Battery {
            percent: cap.trim().parse::<u8>().ok()?.min(100),
            charging: status.trim() == "Charging" || status.trim() == "Full",
        });
    }
    None
}

fn uptime_text(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} d {h} h"),
    }
}

/// Informe en texto para el modelo, con avisos sencillos.
pub fn report(s: &Snapshot) -> String {
    let mut out = vec![
        format!("Sistema: {}", if s.os.is_empty() { "desconocido" } else { &s.os }),
        format!("Procesador: {} ({} hilos) · uso {:.0} %", s.cpu_brand, s.cpu_cores, s.cpu_usage),
        format!(
            "Memoria RAM: {} usados de {} ({} %)",
            gb(s.mem_used),
            gb(s.mem_total),
            pct(s.mem_used, s.mem_total)
        ),
        format!(
            "Memoria comprometida: {} de {} (RAM + archivo de paginación; incluye reservas de la GPU)",
            gb(s.commit_used),
            gb(s.commit_limit)
        ),
        format!("Encendido desde hace: {}", uptime_text(s.uptime_secs)),
    ];
    for d in &s.disks {
        out.push(format!(
            "Disco {}: {} libres de {} ({} % usado)",
            d.name,
            gb(d.free),
            gb(d.total),
            pct(d.total - d.free.min(d.total), d.total)
        ));
    }
    match &s.battery {
        Some(b) => out.push(format!(
            "Batería: {} %{}",
            b.percent,
            if b.charging { " (conectado)" } else { " (sin cargador)" }
        )),
        None => out.push("Batería: no tiene (equipo de escritorio) o no disponible".into()),
    }
    if !s.top.is_empty() {
        out.push("Programas que más memoria usan:".into());
        for p in &s.top {
            out.push(format!(
                "  - {}{}: {} RAM, {:.0} % CPU",
                p.name,
                if p.count > 1 {
                    format!(" ({} procesos)", p.count)
                } else {
                    String::new()
                },
                gb(p.memory),
                p.cpu
            ));
        }
    }
    let mut notes = Vec::new();
    if pct(s.mem_used, s.mem_total) >= 90 {
        notes.push("la memoria RAM está casi llena");
    }
    if s.commit_limit > 0 && s.commit_used * 100 >= s.commit_limit * 90 {
        notes.push("la memoria comprometida está cerca de su límite");
    }
    if s.cpu_usage >= 85.0 {
        notes.push("el procesador está muy ocupado");
    }
    if s.disks.iter().any(|d| pct(d.free, d.total) < 10) {
        notes.push("a un disco le queda menos del 10 % libre");
    }
    if s.battery.as_ref().is_some_and(|b| b.percent <= 15 && !b.charging) {
        notes.push("la batería está baja");
    }
    if s.uptime_secs > 7 * 86_400 {
        notes.push("el equipo lleva más de una semana sin reiniciarse");
    }
    if !notes.is_empty() {
        out.push(format!("Avisos: {}.", notes.join("; ")));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            os: "Windows 11 Pro 24H2".into(),
            cpu_brand: "Intel(R) Core(TM) Ultra 7".into(),
            cpu_cores: 20,
            cpu_usage: 92.0,
            mem_total: 16 * 1024 * 1024 * 1024,
            mem_used: 15 * 1024 * 1024 * 1024,
            commit_used: 46 * 1024 * 1024 * 1024,
            commit_limit: 50 * 1024 * 1024 * 1024,
            uptime_secs: 9 * 86_400,
            disks: vec![DiskInfo {
                name: "C:\\".into(),
                total: 1000 * 1024 * 1024 * 1024,
                free: 50 * 1024 * 1024 * 1024,
            }],
            battery: None,
            top: vec![ProcInfo {
                name: "chrome".into(),
                memory: 3 * 1024 * 1024 * 1024,
                cpu: 12.0,
                count: 24,
            }],
        }
    }

    #[test]
    fn report_is_readable_and_warns() {
        let r = report(&sample());
        assert!(r.contains("15.0 GB usados de 16.0 GB (94 %)"), "{r}");
        assert!(r.contains("Disco C:\\: 50.0 GB libres"));
        assert!(r.contains("chrome (24 procesos): 3.0 GB RAM, 12 % CPU"));
        assert!(r.contains("memoria RAM está casi llena"));
        assert!(r.contains("procesador está muy ocupado"));
        assert!(r.contains("Memoria comprometida: 46.0 GB de 50.0 GB"));
        assert!(r.contains("cerca de su límite"));
        assert!(r.contains("a un disco le queda menos del 10 %"));
        assert!(r.contains("más de una semana"));
        assert!(r.contains("no tiene"));
    }

    #[test]
    fn disk_labels_hide_user_paths() {
        assert_eq!(disk_label("C:\\", 0), "C:\\");
        assert_eq!(disk_label("/", 0), "/");
        assert_eq!(disk_label("/media/eduardo/USB", 2), "unidad 3");
        assert_eq!(disk_label("D:\\Usuarios\\x", 1), "unidad 2");
    }

    #[test]
    fn healthy_system_has_no_warnings() {
        let mut s = sample();
        s.cpu_usage = 10.0;
        s.mem_used = 6 * 1024 * 1024 * 1024;
        s.commit_used = 20 * 1024 * 1024 * 1024;
        s.uptime_secs = 3700;
        s.disks[0].free = 600 * 1024 * 1024 * 1024;
        s.battery = Some(Battery {
            percent: 80,
            charging: false,
        });
        let r = report(&s);
        assert!(!r.contains("Avisos"), "{r}");
        assert!(r.contains("1 h 1 min"));
        assert!(r.contains("Batería: 80 % (sin cargador)"));
    }

    #[test]
    fn real_snapshot_has_no_personal_data() {
        let s = snapshot(true).unwrap();
        assert!(s.mem_total > 0 && s.cpu_cores > 0);
        let r = report(&s);
        let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
        let host = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_default();
        if user.len() > 3 {
            assert!(!r.contains(&format!("/{user}")) && !r.contains(&format!("\\{user}")), "{r}");
        }
        if host.len() > 3 {
            assert!(!r.contains(&host), "{r}");
        }
    }
}
