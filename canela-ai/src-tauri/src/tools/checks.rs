//! Comprobaciones de código (v0.8): un catálogo CERRADO de comandos.
//!
//! - El modelo sólo elige el TIPO de comprobación («sintaxis», «pruebas»,
//!   «compilar») y una ruta dentro de las carpetas autorizadas. El programa
//!   decide el comando a partir de los archivos del proyecto; nunca se ejecuta
//!   texto generado por el modelo.
//! - Sin intérprete de comandos: el programa se lanza directamente, con
//!   argumentos fijos, en la carpeta del proyecto.
//! - Variables de entorno con aspecto de secreto (KEY, TOKEN, SECRET,
//!   PASSWORD…) se quitan antes de lanzar el proceso.
//! - Límite de tiempo; si se supera o se cancela la respuesta, el proceso se
//!   termina. La salida se recorta, se ocultan claves y se trata como datos.
//! - Las comprobaciones que ejecutan código del proyecto (pruebas, compilar)
//!   siempre piden permiso mostrando el comando exacto. La sintaxis de .js y
//!   .py usa Node/Python SIN ejecutar el programa; los demás formatos se
//!   revisan dentro de Canela, sin lanzar ningún proceso.

use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Sintaxis,
    Pruebas,
    Compilar,
}

pub fn parse_kind(s: &str) -> Result<Kind, String> {
    match s.trim().to_lowercase().as_str() {
        "sintaxis" => Ok(Kind::Sintaxis),
        "pruebas" | "tests" | "test" => Ok(Kind::Pruebas),
        "compilar" | "compilacion" | "compilación" | "build" => Ok(Kind::Compilar),
        other => Err(format!(
            "Tipo de comprobación «{other}» no admitido. Sólo: sintaxis, pruebas o compilar."
        )),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckCmd {
    /// Programa a lanzar (ruta completa). Vacío = revisión interna.
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Cómo se muestra al usuario («npm test»).
    pub display: String,
    /// ¿Ejecuta código del proyecto? (pruebas, compilación)
    pub runs_project_code: bool,
    pub timeout: Duration,
    /// Revisión interna de este archivo (sin procesos).
    pub internal: Option<PathBuf>,
    /// Quitar del entorno las variables con aspecto de secreto (siempre en la
    /// app; sólo se desactiva en el diagnóstico de pruebas).
    pub scrub_env: bool,
    /// Lanzar sin ventana de consola (Windows).
    pub hide_window: bool,
    /// Fijar el PATH del proceso (siempre en la app; el diagnóstico lo
    /// desactiva para comparar con el entorno heredado tal cual).
    pub set_path: bool,
    /// Longitud máxima de ese PATH (en Windows).
    pub path_limit: usize,
}

/// Busca un programa en el PATH (en Windows también .exe, .cmd y .bat).
pub fn which(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    // En Windows sólo ejecutables reales (no el script sin extensión que npm
    // también instala junto a npm.cmd).
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ".com"] } else { &[""] };
    for dir in std::env::split_paths(&path) {
        for n in names {
            for e in exts {
                let p = dir.join(format!("{n}{e}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

const PY_AST: &str = "import ast,sys;ast.parse(open(sys.argv[1],encoding='utf-8').read(),sys.argv[1])";
const NPM_DEFAULT_TEST: &str = "no test specified";

fn package_script(dir: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["scripts"][name].as_str().map(str::to_string)
}

fn need(names: &[&str], what: &str) -> Result<PathBuf, String> {
    which(names).ok_or_else(|| format!("No encontré {what} instalado en este equipo; no puedo hacer esta comprobación."))
}

/// En Windows, las rutas normalizadas tienen la forma `\\?\C:\…`, que
/// Node, npm y cmd.exe no aceptan bien: se pasa la ruta normal `C:\…`.
pub fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// Longitud máxima del PATH del proceso. En Windows, cmd.exe (que usa npm
/// para lanzar las pruebas) deja de usar el PATH si supera 8 191 caracteres.
const MAX_PATH_LEN: usize = 8000;
/// Límite de cmd.exe para una variable de entorno.
const CMD_MAX: usize = 8191;

/// Cuánto PATH se puede pasar sin que, al añadir npm sus carpetas, se supere
/// el límite de cmd.exe. Comprobado en Windows: con 5 857 caracteres npm
/// encuentra `node`; con 7 956, no (npm antepone `node_modules\.bin` de cada
/// carpeta superior del proyecto y la suya propia).
pub fn path_budget(cwd: &Path) -> usize {
    let npm_bins: usize = cwd.ancestors().map(|a| a.as_os_str().len() + "\\node_modules\\.bin;".len()).sum();
    // node-gyp-bin de npm y otros añadidos; margen holgado.
    let margin = 600;
    CMD_MAX.saturating_sub(npm_bins + margin).clamp(1024, MAX_PATH_LEN)
}

/// PATH del proceso: primero la carpeta del programa, la de Node y las de
/// Windows; después el PATH heredado (sin repetidos) hasta el límite. Así las
/// pruebas que lanza npm encuentran `node` aunque el PATH heredado sea enorme.
fn child_path(program: &Path, max: usize) -> Option<std::ffi::OsString> {
    let mut first: Vec<PathBuf> = [
        program.parent().map(Path::to_path_buf),
        which(&["node"]).and_then(|n| n.parent().map(Path::to_path_buf)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let root = PathBuf::from(root);
            first.push(root.join("System32"));
            first.push(root.clone());
            first.push(root.join("System32").join("Wbem"));
        }
    }
    let inherited: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let limit = if cfg!(windows) { Some(max) } else { None };
    std::env::join_paths(build_path(first, inherited, limit)).ok()
}

/// Lista final de carpetas: primero `first`, después las heredadas, sin
/// vacías ni repetidas y, si hay límite, sin pasarse de él (las primeras
/// nunca se pierden porque van delante).
fn build_path(first: Vec<PathBuf>, inherited: Vec<PathBuf>, limit: Option<usize>) -> Vec<PathBuf> {
    let mut parts: Vec<PathBuf> = Vec::new();
    let mut len = 0;
    for p in first.into_iter().chain(inherited) {
        if p.as_os_str().is_empty() || parts.contains(&p) {
            continue;
        }
        let l = p.as_os_str().len() + 1;
        if limit.is_some_and(|max| len + l > max) {
            break;
        }
        len += l;
        parts.push(p);
    }
    parts
}

/// Decide el comando (o la revisión interna) para una comprobación.
pub fn plan(kind: Kind, target: &Path) -> Result<CheckCmd, String> {
    let target = &plain(target);
    let secs = |s| Duration::from_secs(s);
    match kind {
        Kind::Sintaxis => {
            if !target.is_file() {
                return Err("Para revisar la sintaxis indica un archivo, no una carpeta.".into());
            }
            let ext = target.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let cwd = target.parent().unwrap_or(Path::new(".")).to_path_buf();
            let file = target.to_string_lossy().to_string();
            let name = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            match ext.as_str() {
                "js" | "mjs" | "cjs" => Ok(CheckCmd {
                    program: need(&["node"], "Node.js")?,
                    args: vec!["--check".into(), file],
                    cwd,
                    display: format!("node --check {name}"),
                    runs_project_code: false,
                    timeout: secs(30),
                    internal: None,
                    scrub_env: true,
                    hide_window: true,
                    set_path: true,
                    path_limit: MAX_PATH_LEN,
                }),
                "py" => Ok(CheckCmd {
                    program: need(&["python", "python3", "py"], "Python")?,
                    args: vec!["-c".into(), PY_AST.into(), file],
                    cwd,
                    display: format!("Python: revisar la sintaxis de {name} (sin ejecutarlo)"),
                    runs_project_code: false,
                    timeout: secs(30),
                    internal: None,
                    scrub_env: true,
                    hide_window: true,
                    set_path: true,
                    path_limit: MAX_PATH_LEN,
                }),
                _ => Ok(CheckCmd {
                    program: PathBuf::new(),
                    args: vec![],
                    cwd,
                    display: format!("revisión interna de la sintaxis de {name}"),
                    runs_project_code: false,
                    timeout: secs(5),
                    internal: Some(target.to_path_buf()),
                    scrub_env: true,
                    hide_window: true,
                    set_path: true,
                    path_limit: MAX_PATH_LEN,
                }),
            }
        }
        Kind::Pruebas | Kind::Compilar => {
            let dir = if target.is_file() {
                target.parent().unwrap_or(target)
            } else {
                target
            }
            .to_path_buf();
            let has = |f: &str| dir.join(f).exists();
            let script = if kind == Kind::Pruebas { "test" } else { "build" };
            let cmd = |program: PathBuf, args: &[&str], display: &str| CheckCmd {
                program,
                args: args.iter().map(|a| a.to_string()).collect(),
                cwd: dir.clone(),
                display: display.to_string(),
                runs_project_code: true,
                timeout: secs(300),
                internal: None,
                scrub_env: true,
                hide_window: true,
                set_path: true,
                path_limit: MAX_PATH_LEN,
            };
            if let Some(s) = package_script(&dir, script).filter(|s| !s.contains(NPM_DEFAULT_TEST)) {
                let _ = s;
                let npm = need(&["npm"], "npm (Node.js)")?;
                return Ok(if kind == Kind::Pruebas {
                    cmd(npm, &["test"], "npm test")
                } else {
                    cmd(npm, &["run", "build"], "npm run build")
                });
            }
            if has("Cargo.toml") {
                let cargo = need(&["cargo"], "Rust (cargo)")?;
                return Ok(if kind == Kind::Pruebas {
                    cmd(cargo, &["test"], "cargo test")
                } else {
                    cmd(cargo, &["check"], "cargo check")
                });
            }
            if kind == Kind::Pruebas && (has("pytest.ini") || has("pyproject.toml") || has("setup.cfg") || has("tests")) {
                let py = need(&["python", "python3", "py"], "Python")?;
                return Ok(cmd(py, &["-m", "pytest", "-q"], "python -m pytest -q"));
            }
            let tsc = dir.join("node_modules").join("typescript").join("bin").join("tsc");
            if kind == Kind::Compilar && has("tsconfig.json") && tsc.is_file() {
                let node = need(&["node"], "Node.js")?;
                let mut c = cmd(node, &[], "tsc --noEmit (TypeScript del proyecto)");
                c.args = vec![tsc.to_string_lossy().to_string(), "--noEmit".into()];
                return Ok(c);
            }
            Err(if kind == Kind::Pruebas {
                "No encontré pruebas configuradas en esa carpeta (package.json con «test», Cargo.toml o pytest).".into()
            } else {
                "No encontré cómo compilar esa carpeta (package.json con «build», Cargo.toml o tsconfig.json con TypeScript instalado)."
                    .into()
            })
        }
    }
}

fn looks_secret(name: &str) -> bool {
    let n = name.to_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "CANELA_DEV"]
        .iter()
        .any(|k| n.contains(k))
}

/// Resultado de una comprobación.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    pub code: Option<i32>,
    pub output: String,
}

const MAX_OUTPUT: usize = 3000;

fn tail(s: &str) -> String {
    let n = s.chars().count();
    if n <= MAX_OUTPUT {
        s.to_string()
    } else {
        format!("[…]{}", s.chars().skip(n - MAX_OUTPUT).collect::<String>())
    }
}

/// Termina el proceso y TODOS los que lanzó (npm lanza node, cargo lanza
/// rustc…). Comando fijo del sistema, sin intérprete.
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // El proceso se lanzó en su propio grupo: se termina el grupo entero.
        let _ = std::process::Command::new("kill")
            // «--» para que «-PID» se lea como grupo de procesos y no como señal.
            .args(["-s", "KILL", "--", &format!("-{pid}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Si la respuesta se cancela a mitad, el árbol de procesos se termina.
struct TreeGuard {
    pid: Option<u32>,
}

impl Drop for TreeGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid.take() {
            kill_tree(pid);
        }
    }
}

/// Ejecuta la comprobación. Si la respuesta se cancela, el proceso termina.
pub async fn run(cmd: &CheckCmd) -> Result<Outcome, String> {
    if let Some(file) = &cmd.internal {
        let content = std::fs::read_to_string(file).map_err(|e| format!("No pude leer el archivo: {e}"))?;
        let problems = crate::verification::code_check(&file.to_string_lossy(), &content);
        return Ok(Outcome {
            ok: problems.is_empty(),
            code: None,
            output: if problems.is_empty() {
                "Sin errores de sintaxis evidentes (revisión básica; no ejecuta el código).".into()
            } else {
                problems.join("\n")
            },
        });
    }
    let mut c = tokio::process::Command::new(&cmd.program);
    c.args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if cmd.scrub_env {
        for (k, _) in std::env::vars_os() {
            if looks_secret(&k.to_string_lossy()) {
                c.env_remove(&k);
            }
        }
    }
    if cmd.set_path {
        let limit = cmd.path_limit.min(path_budget(&cmd.cwd));
        if let Some(path) = child_path(&cmd.program, limit) {
            c.env("PATH", path);
        }
    }
    #[cfg(windows)]
    {
        // Sin ventana de consola.
        if cmd.hide_window {
            c.creation_flags(0x0800_0000);
        }
    }
    #[cfg(unix)]
    {
        c.process_group(0);
    }
    let child = c.spawn().map_err(|e| format!("No pude iniciar la comprobación: {e}"))?;
    let mut guard = TreeGuard { pid: child.id() };
    let result = tokio::time::timeout(cmd.timeout, child.wait_with_output()).await;
    if result.is_ok() {
        // Terminó solo: puede que deje procesos hijos; también se terminan.
        if let Some(pid) = guard.pid.take() {
            kill_tree(pid);
        }
    }
    drop(guard);
    let out = match result {
        Ok(r) => r.map_err(|e| format!("La comprobación falló al ejecutarse: {e}"))?,
        Err(_) => {
            return Ok(Outcome {
                ok: false,
                code: None,
                output: format!("Tardó más de {} s y se detuvo.", cmd.timeout.as_secs()),
            })
        }
    };
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    Ok(Outcome {
        ok: out.status.success(),
        code: out.status.code(),
        output: crate::ai::pricing::redact_secrets(&tail(text.trim())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("canela-checks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_known_kinds_are_accepted() {
        assert_eq!(parse_kind("pruebas").unwrap(), Kind::Pruebas);
        for bad in ["rm -rf /", "powershell", "npm install", "", "ejecutar"] {
            assert!(parse_kind(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_command_comes_from_the_project_not_from_the_model() {
        let d = dir("detect");
        assert!(plan(Kind::Pruebas, &d).unwrap_err().contains("No encontré pruebas"));
        // El «test» por defecto de npm no cuenta como pruebas.
        std::fs::write(
            d.join("package.json"),
            r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
        )
        .unwrap();
        assert!(plan(Kind::Pruebas, &d).is_err());
        if which(&["npm"]).is_some() {
            std::fs::write(d.join("package.json"), r#"{"scripts":{"test":"node t.js","build":"node b.js"}}"#).unwrap();
            let p = plan(Kind::Pruebas, &d).unwrap();
            assert_eq!(
                (p.display.as_str(), p.args.clone(), p.runs_project_code),
                ("npm test", vec!["test".to_string()], true)
            );
            assert_eq!(plan(Kind::Compilar, &d).unwrap().display, "npm run build");
        }
        let f = d.join("datos.json");
        std::fs::write(&f, "{}").unwrap();
        let p = plan(Kind::Sintaxis, &f).unwrap();
        assert_eq!(p.internal.as_deref(), Some(f.as_path()), "JSON se revisa dentro de Canela");
        assert!(plan(Kind::Sintaxis, &d).is_err(), "sintaxis necesita un archivo");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn secret_looking_variables_are_removed() {
        for k in ["OPENAI_API_KEY", "GITHUB_TOKEN", "DB_PASSWORD", "MY_SECRET", "CANELA_DEV_API_KEY"] {
            assert!(looks_secret(k), "{k}");
        }
        for k in ["PATH", "HOME", "LANG", "NODE_ENV"] {
            assert!(!looks_secret(k), "{k}");
        }
    }

    #[test]
    fn child_path_puts_node_first_and_stays_short_on_windows() {
        let program = PathBuf::from("/opt/herramienta/bin/npm");
        let p = child_path(&program, MAX_PATH_LEN).unwrap();
        let parts: Vec<PathBuf> = std::env::split_paths(&p).collect();
        assert_eq!(parts[0], PathBuf::from("/opt/herramienta/bin"));
        if cfg!(windows) {
            assert!(p.len() <= MAX_PATH_LEN, "{}", p.len());
        }
    }

    #[test]
    fn reduced_path_keeps_needed_tools_first_and_everything_when_short() {
        let first = vec![PathBuf::from("C:/Program Files/nodejs"), PathBuf::from("C:/Windows/System32")];
        // PATH normal: no se pierde nada, sin repetidos ni vacíos, en orden.
        let normal = vec![
            PathBuf::from("C:/Program Files/Git/cmd"),
            PathBuf::new(),
            PathBuf::from("C:/Program Files/nodejs"),
            PathBuf::from("C:/Users/Ana María/AppData/Local/Programs/Python/Python312"),
        ];
        let out = build_path(first.clone(), normal, Some(MAX_PATH_LEN));
        assert_eq!(
            out,
            vec![
                PathBuf::from("C:/Program Files/nodejs"),
                PathBuf::from("C:/Windows/System32"),
                PathBuf::from("C:/Program Files/Git/cmd"),
                PathBuf::from("C:/Users/Ana María/AppData/Local/Programs/Python/Python312"),
            ]
        );
        // PATH enorme (como el de «cargo test»): se recorta, pero lo necesario
        // va primero y las rutas con espacios quedan intactas.
        let huge: Vec<PathBuf> = (0..600)
            .map(|i| PathBuf::from(format!("D:/a/target/debug/build/carpeta con espacios {i:04}/out/build")))
            .collect();
        let out = build_path(first.clone(), huge, Some(MAX_PATH_LEN));
        let total: usize = out.iter().map(|p| p.as_os_str().len() + 1).sum();
        assert!(total <= MAX_PATH_LEN, "{total}");
        assert_eq!(&out[..2], &first[..]);
        assert!(out[2].to_string_lossy().contains("carpeta con espacios 0000"));
        // Sin límite (Linux/macOS) no se recorta nada.
        let many: Vec<PathBuf> = (0..600).map(|i| PathBuf::from(format!("/x/{i}"))).collect();
        assert_eq!(build_path(vec![], many, None).len(), 600);
    }

    #[test]
    fn detected_tools_are_really_runnable() {
        // Lo que encuentra `which` debe poder ejecutarse (npm.cmd en Windows).
        for (names, what) in [
            (&["node"][..], "node"),
            (&["npm"][..], "npm"),
            (&["python", "python3", "py"][..], "python"),
        ] {
            match which(names) {
                Some(p) => {
                    let out = std::process::Command::new(&p).arg("--version").output();
                    let ok = out.as_ref().is_ok_and(|o| o.status.success());
                    assert!(ok, "{what} en {p:?} no se pudo ejecutar: {out:?}");
                    eprintln!("{what}: {p:?} → {}", String::from_utf8_lossy(&out.unwrap().stdout).trim());
                }
                None => eprintln!("{what}: no instalado"),
            }
        }
    }

    #[tokio::test]
    async fn checks_work_in_folders_with_spaces_and_accents() {
        let d = dir("con espacios y ñ").join("Mi Proyecto Ñandú");
        std::fs::create_dir_all(&d).unwrap();
        if which(&["node"]).is_some() {
            let f = d.join("mi app.js");
            std::fs::write(&f, "const x = 1;\n").unwrap();
            let o = run(&plan(Kind::Sintaxis, &f).unwrap()).await.unwrap();
            assert!(o.ok, "node --check con espacios: {o:?}");
        }
        if which(&["python", "python3", "py"]).is_some() {
            let f = d.join("mi script.py");
            std::fs::write(&f, "x = 1\n").unwrap();
            let o = run(&plan(Kind::Sintaxis, &f).unwrap()).await.unwrap();
            assert!(o.ok, "python con espacios: {o:?}");
        }
        if which(&["npm"]).is_some() && which(&["node"]).is_some() {
            std::fs::write(d.join("prueba uno.js"), "console.log('ok con espacios')").unwrap();
            std::fs::write(
                d.join("package.json"),
                r#"{"name":"x","version":"1.0.0","scripts":{"test":"node \"prueba uno.js\""}}"#,
            )
            .unwrap();
            let o = run(&plan(Kind::Pruebas, &d).unwrap()).await.unwrap();
            assert!(o.ok && o.output.contains("ok con espacios"), "npm test con espacios: {o:?}");
        }
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    /// Diagnóstico (no oculta fallos: sólo informa). Lanza `npm test` con un
    /// script que muestra el entorno que recibe cmd.exe/sh, con cada factor
    /// activado o no, para identificar la causa exacta de un fallo en Windows.
    #[tokio::test]
    async fn diagnostico_entorno_de_npm() {
        if which(&["npm"]).is_none() || which(&["node"]).is_none() {
            eprintln!("(sin npm/node)");
            return;
        }
        // 1) Variables de ruta del propio proceso de pruebas.
        for (k, v) in std::env::vars_os() {
            if k.to_string_lossy().eq_ignore_ascii_case("path") {
                eprintln!("proceso de pruebas: {:?} ({} caracteres)", k, v.len());
            }
        }
        let d = dir("diag");
        let script = if cfg!(windows) {
            "set & where node"
        } else {
            "env; command -v node"
        };
        std::fs::write(
            d.join("package.json"),
            format!(r#"{{"name":"x","version":"1.0.0","scripts":{{"test":{script:?}}}}}"#),
        )
        .unwrap();
        let npm = plan(Kind::Pruebas, &d).unwrap();
        // node.exe directamente (sin npm): qué variables de ruta recibe.
        let mut probe = npm.clone();
        probe.program = which(&["node"]).unwrap();
        probe.args = vec![
            "-e".into(),
            "console.log('node ve: ' + JSON.stringify(Object.keys(process.env).filter(k => /^path$/i.test(k)).map(k => k + '(' + process.env[k].length + ')')))".into(),
        ];
        eprintln!("npm: {:?}; node: {:?}", npm.program, probe.program);
        // (limpiar entorno, fijar PATH, sin ventana)
        for (scrub, set_path, hide) in [
            (true, true, true),
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (false, false, true),
        ] {
            for (what, base) in [("node directo", &probe), ("npm test", &npm)] {
                let mut c = base.clone();
                c.scrub_env = scrub;
                c.set_path = set_path;
                c.hide_window = hide;
                let o = run(&c).await.unwrap();
                eprintln!(
                    "== {what}: limpiar_entorno={scrub} fijar_path={set_path} sin_ventana={hide} → ok={} código={:?}",
                    o.ok, o.code
                );
                for l in o.output.lines() {
                    let low = l.to_lowercase();
                    let key = low.split('=').next().unwrap_or("");
                    if ["path", "pathext", "comspec"].contains(&key) || low.contains("node") || low.contains("info:") {
                        eprintln!("   {}", l.chars().take(160).collect::<String>());
                    }
                }
            }
        }
        // 2) Lo que confirmó la causa (Windows): con PATH ≤ 6 000 npm
        //    encuentra node; con 7 956, no. Se muestra para seguimiento.
        std::fs::write(d.join("t.js"), "console.log('NODE ENCONTRADO')").unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"x","version":"1.0.0","scripts":{"test":"node t.js"}}"#,
        )
        .unwrap();
        let npm = plan(Kind::Pruebas, &d).unwrap();
        for limit in [2000, 6000, 8000] {
            let mut c = npm.clone();
            c.path_limit = limit;
            let o = run(&c).await.unwrap();
            eprintln!("== npm test con PATH ≤ {limit} (presupuesto {}) → ok={}", path_budget(&c.cwd), o.ok);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn path_budget_leaves_room_for_what_npm_adds() {
        let shallow = path_budget(Path::new(r"C:\Proyectos\web"));
        let deep = PathBuf::from(r"C:\Users\Ana María\Documents\Mis Proyectos Ñandú\cliente grande\frontend\paquetes\interfaz");
        let deep_budget = path_budget(&deep);
        assert!(deep_budget < shallow, "más profundo → menos presupuesto");
        // Presupuesto + lo que añade npm (con margen) nunca supera cmd.exe.
        let npm_adds: usize = deep.ancestors().map(|a| a.as_os_str().len() + 20).sum();
        assert!(deep_budget + npm_adds + 300 <= CMD_MAX, "{deep_budget} + {npm_adds}");
        // El valor que falló en Windows (7 956) ya no se usa con una ruta típica.
        let ci = path_budget(Path::new(r"C:\Users\RUNNER~1\AppData\Local\Temp\canela-checks-diag-8488"));
        assert!((6000..7956).contains(&ci), "{ci}");
    }

    /// Regresión del fallo original: con el PATH enorme de «cargo test» (más
    /// de 20 000 caracteres en Windows), npm test no encontraba node. Debe
    /// funcionar en una carpeta profunda, con espacios y acentos.
    #[tokio::test]
    async fn npm_finds_node_in_deep_folders_with_spaces_regression() {
        if which(&["npm"]).is_none() || which(&["node"]).is_none() {
            eprintln!("(sin npm: prueba omitida)");
            return;
        }
        let d = dir("regresion")
            .join("Mis Proyectos Ñandú")
            .join("cliente grande")
            .join("frontend web");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("prueba uno.js"), "console.log('node encontrado: ' + process.version)").unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"x","version":"1.0.0","scripts":{"test":"node \"prueba uno.js\""}}"#,
        )
        .unwrap();
        let o = run(&plan(Kind::Pruebas, &d).unwrap()).await.unwrap();
        assert!(o.ok && o.output.contains("node encontrado: v"), "{o:?}");
        let _ = std::fs::remove_dir_all(dir("regresion"));
    }

    #[test]
    fn verbatim_windows_paths_become_plain() {
        assert_eq!(plain(Path::new(r"\\?\C:\Users\x\app.js")), PathBuf::from(r"C:\Users\x\app.js"));
        // Rutas de red (\\?\UNC\…) y normales no se tocan.
        assert_eq!(plain(Path::new(r"\\?\UNC\srv\x")), PathBuf::from(r"\\?\UNC\srv\x"));
        assert_eq!(plain(Path::new("/home/x/app.js")), PathBuf::from("/home/x/app.js"));
    }

    #[tokio::test]
    async fn internal_syntax_check_runs_no_process() {
        let d = dir("internal");
        let f = d.join("c.json");
        std::fs::write(&f, r#"{"a": 1,}"#).unwrap();
        let o = run(&plan(Kind::Sintaxis, &f).unwrap()).await.unwrap();
        assert!(!o.ok && o.output.contains("JSON no válido"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn node_syntax_check_does_not_run_the_program() {
        if which(&["node"]).is_none() {
            eprintln!("(sin Node.js: prueba omitida)");
            return;
        }
        let d = dir("node");
        let marker = d.join("ejecutado.txt");
        let good = d.join("ok.js");
        // Si se EJECUTARA, crearía el archivo marcador.
        std::fs::write(
            &good,
            format!("require('fs').writeFileSync({:?}, 'x');\n", marker.to_string_lossy()),
        )
        .unwrap();
        let o = run(&plan(Kind::Sintaxis, &good).unwrap()).await.unwrap();
        assert!(o.ok, "{o:?}");
        assert!(!marker.exists(), "--check no ejecuta el programa");
        let bad = d.join("mal.js");
        std::fs::write(&bad, "function f( {\n").unwrap();
        let o = run(&plan(Kind::Sintaxis, &bad).unwrap()).await.unwrap();
        assert!(!o.ok && o.code == Some(1), "{o:?}");
        assert!(o.output.contains("SyntaxError"), "{}", o.output);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn python_syntax_check_does_not_run_the_program() {
        if which(&["python", "python3", "py"]).is_none() {
            eprintln!("(sin Python: prueba omitida)");
            return;
        }
        let d = dir("py");
        let marker = d.join("ejecutado.txt");
        let good = d.join("ok.py");
        std::fs::write(&good, format!("open({:?}, 'w').write('x')\n", marker.to_string_lossy())).unwrap();
        let o = run(&plan(Kind::Sintaxis, &good).unwrap()).await.unwrap();
        assert!(o.ok, "{o:?}");
        assert!(!marker.exists(), "sólo se analiza, no se ejecuta");
        let bad = d.join("mal.py");
        std::fs::write(&bad, "def f(:\n    pass\n").unwrap();
        let o = run(&plan(Kind::Sintaxis, &bad).unwrap()).await.unwrap();
        assert!(!o.ok, "{o:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn project_tests_run_with_a_time_limit_and_without_secrets() {
        if which(&["npm"]).is_none() || which(&["node"]).is_none() {
            eprintln!("(sin npm: prueba omitida)");
            return;
        }
        let d = dir("npm");
        // La prueba falla si ve la clave en su entorno.
        std::fs::write(
            d.join("t.js"),
            "if (process.env.CANELA_FAKE_API_KEY) { console.log('VIO LA CLAVE'); process.exit(2) } console.log('1 prueba ok')",
        )
        .unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"x","version":"1.0.0","scripts":{"test":"node t.js"}}"#,
        )
        .unwrap();
        let guard = crate::TEST_ENV_LOCK.lock().await;
        std::env::set_var("CANELA_FAKE_API_KEY", "sk-no-debe-verse");
        let o = run(&plan(Kind::Pruebas, &d).unwrap()).await.unwrap();
        std::env::remove_var("CANELA_FAKE_API_KEY");
        drop(guard);
        assert!(
            o.ok,
            "{o:?}\nPATH del proceso: {:?}",
            child_path(&plan(Kind::Pruebas, &d).unwrap().program, MAX_PATH_LEN)
        );
        assert!(o.output.contains("1 prueba ok") && !o.output.contains("VIO LA CLAVE"));
        // Límite de tiempo: el proceso de las pruebas (hijo de npm) también termina.
        let pidfile = d.join("pid.txt");
        std::fs::write(
            d.join("t.js"),
            format!(
                "require('fs').writeFileSync({:?}, String(process.pid)); setTimeout(() => {{}}, 60000)",
                pidfile.to_string_lossy()
            ),
        )
        .unwrap();
        let mut c = plan(Kind::Pruebas, &d).unwrap();
        c.timeout = Duration::from_secs(3);
        let started = std::time::Instant::now();
        let o = run(&c).await.unwrap();
        assert!(!o.ok && o.output.contains("se detuvo"), "{o:?}");
        assert!(started.elapsed() < Duration::from_secs(20));
        #[cfg(target_os = "linux")]
        {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let pid = std::fs::read_to_string(&pidfile).unwrap();
            // Un proceso terminado puede quedar como «zombi» (estado Z) hasta
            // que lo recoja su padre: eso no es un proceso vivo.
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim())).unwrap_or_default();
            let state = stat.rsplit(')').next().and_then(|r| r.split_whitespace().next()).unwrap_or("");
            assert!(stat.is_empty() || state == "Z", "quedó vivo el proceso {pid} (estado {state})");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
