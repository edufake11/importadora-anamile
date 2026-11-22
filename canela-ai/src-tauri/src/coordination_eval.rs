//! Evaluación del planificador (v0.7.2) con solicitudes NUEVAS.
//!
//! Escritas después de terminar las reglas, separadas de las usadas para
//! desarrollarlas, y medidas una sola vez: los fallos que aparezcan aquí se
//! documentan como pendientes, no se corrigen ajustando las reglas a este
//! conjunto (eso lo volvería optimista).
//!
//! Etiquetas (hechas a mano, con el resultado IDEAL, no el que dan las reglas):
//! - `P(&[...])`: plan con esos agentes en orden (`Inf` = paso de informe).
//! - `S`: lo atiende un solo agente (no hace falta plan).
//! - `C`: hay que pedir aclaración antes de hacer nada.

use crate::agents::AgentId::{self, *};
use crate::coordination::{analyze, Analysis};

#[derive(Debug, Clone, Copy)]
enum E {
    P(&'static [AgentId]),
    S,
    C,
}
use E::*;

const INF: AgentId = General;

const SET: &[(&str, E)] = &[
    (
        "revisa el código de mi app de Flask y crea una tarea por cada bug que encuentres",
        P(&[Programador, Productividad]),
    ),
    (
        "primero mira cuánta RAM estoy usando, luego apunta una tarea para cerrar programas que no uso",
        P(&[Sistema, Productividad]),
    ),
    (
        "busca en Wikipedia quién inventó el telescopio y guárdamelo en un archivo en el escritorio",
        P(&[Investigador, Archivos]),
    ),
    (
        "Por favor revise el script de Python y anote una tarea con los errores",
        P(&[Programador, Productividad]),
    ),
    ("ordena mi carpeta de descargas y después hazme un resumen de lo que moviste", S),
    (
        "chécame la batería y agéndame una tarea para comprar cargador",
        P(&[Sistema, Productividad]),
    ),
    (
        "lee el PDF del contrato de arrendamiento, explícame las cláusulas importantes y crea una tarea para firmarlo",
        P(&[Archivos, Productividad]),
    ),
    (
        "investiga qué es Docker, revisa mi Dockerfile y prepara un informe",
        P(&[Investigador, Programador, INF]),
    ),
    (
        "diagnostica por qué el disco está lleno y organiza los archivos grandes de descargas",
        P(&[Sistema, Archivos]),
    ),
    (
        "1. revisa los errores de compilación\n2. crea tareas para cada uno\n3. hazme un informe",
        P(&[Programador, Productividad, INF]),
    ),
    (
        "abre mi proyecto en VS Code y pon música para concentrarme",
        P(&[Programador, Productividad]),
    ),
    (
        "averigua cuándo se fundó la UNAM y crea una tarea para estudiar eso el lunes",
        P(&[Investigador, Productividad]),
    ),
    (
        "mira qué procesos consumen más CPU y guárdame la lista en un archivo de texto",
        P(&[Sistema, Archivos]),
    ),
    (
        "busca mis fotos de vacaciones, ordénalas por fecha y crea una tarea para imprimirlas",
        P(&[Archivos, Productividad]),
    ),
    (
        "revisa la función de pago en el código, corrígela y después documenta lo que cambiaste",
        S,
    ),
    (
        "lee la página https://es.wikipedia.org/wiki/Teckel y crea una tarea para comprar comida de perro",
        P(&[Investigador, Productividad]),
    ),
    (
        "quiero que revises el rendimiento de la computadora y que me organices la carpeta de documentos",
        P(&[Sistema, Archivos]),
    ),
    (
        "dime cuánto espacio libre tengo en el disco y recuérdame mañana borrar archivos viejos",
        P(&[Sistema, Productividad]),
    ),
    (
        "analiza el archivo ventas.csv, saca conclusiones y crea tareas de seguimiento",
        P(&[Archivos, Productividad]),
    ),
    (
        "investiga los síntomas de la gripe y anota una tarea para ir al médico",
        P(&[Investigador, Productividad]),
    ),
    (
        "revisa el repositorio, haz una lista de bugs y guárdala en un archivo del escritorio",
        P(&[Programador, Archivos]),
    ),
    ("pausa la música y dime por qué la PC está tan lenta", P(&[Productividad, Sistema])),
    (
        "Organiza mis tareas pendientes de la semana y busca el documento del proyecto final",
        P(&[Productividad, Archivos]),
    ),
    (
        "revisa mi código de JavaScript, investiga en internet la mejor práctica y crea una tarea",
        P(&[Programador, Investigador, Productividad]),
    ),
    (
        "comprueba la temperatura del procesador y si está alta crea una tarea para limpiar el ventilador",
        P(&[Sistema, Productividad]),
    ),
    ("busca el Excel del presupuesto y explícame en qué gasto más", S),
    ("investiga la historia del chocolate y hazme un resumen", S),
    ("crea una tarea para estudiar y luego pon música", S),
    ("revisa el código y corrige los errores, luego ejecuta las pruebas", S),
    ("organiza mis descargas por tipo y luego borra las carpetas vacías", S),
    ("¿qué tengo pendiente hoy? y pon música tranquila", S),
    ("lee el informe de ventas y resúmelo", S),
    ("hola Canela, ¿cómo estás?", S),
    ("abre la calculadora", S),
    ("cuéntame un chiste y luego pon música", S),
    ("explícame qué es la recursividad", S),
    ("dime la hora", S),
    ("revisa la memoria del equipo", S),
    ("crea una carpeta llamada Proyectos en el escritorio", S),
    ("busca información sobre los volcanes de México", S),
    ("Haz una lista de compras y guárdala como tarea", S),
    ("lee mi archivo de notas y dime qué tareas tengo apuntadas ahí", S),
    ("analiza el código de mi página y dime si tiene errores", S),
    ("mira la batería y el disco", S),
    (
        "crea la carpeta Respaldo en el escritorio, revisa el código y no crees ninguna carpeta",
        C,
    ),
    ("revisa mis descargas y bórralo todo", C),
    ("busca los archivos viejos, elimina todo eso y apunta una tarea", C),
    ("pon música y no pongas música", C),
    ("organiza el escritorio y luego vacíalo", C),
    ("revisa la memoria, limpia todo y crea una tarea", C),
    (
        "investiga el clima de mañana pero no investigues nada en internet, y crea una tarea",
        C,
    ),
    ("mueve eso a la otra carpeta y crea un recordatorio", C),
    ("revisa los errores del proyecto, crea las tareas y no crees tareas nuevas", C),
    (
        "Busca en internet qué es el IVA y después crea un documento en mi escritorio con la explicación",
        P(&[Investigador, Archivos]),
    ),
    (
        "Revisa los logs del servidor, identifica el error y apúntalo como tarea urgente",
        P(&[Programador, Productividad]),
    ),
    (
        "mira si mi PC tiene poca memoria y, si es así, apunta que debo comprar más RAM",
        P(&[Sistema, Productividad]),
    ),
    (
        "lee el capítulo 3 del PDF de biología y crea tareas de repaso para cada tema",
        P(&[Archivos, Productividad]),
    ),
    (
        "investiga cómo funciona Git, luego revisa mi repositorio y dime qué commits faltan",
        P(&[Investigador, Programador]),
    ),
    (
        "silencia la música, revisa el uso de CPU y anota en una tarea qué programa consume más",
        P(&[Productividad, Sistema, Productividad]),
    ),
    (
        "Encuentra el archivo del CV, revísalo y recuérdame enviarlo el viernes",
        P(&[Archivos, Productividad]),
    ),
    (
        "dime qué proceso está usando la GPU y busca información sobre cómo bajar la temperatura",
        P(&[Sistema, Investigador]),
    ),
    (
        "crea una carpeta para el curso de Python, y luego escribe un script de ejemplo dentro",
        P(&[Archivos, Programador]),
    ),
    (
        "investiga recetas con chocolate y agrégalas a mi lista de compras",
        P(&[Investigador, Productividad]),
    ),
    (
        "revisa la memoria y el disco, y después organiza mis descargas",
        P(&[Sistema, Archivos]),
    ),
    (
        "analiza mi proyecto de React y busca en Wikipedia qué es el Virtual DOM",
        P(&[Programador, Investigador]),
    ),
    (
        "toma nota: llamar al banco. Luego revisa si hay espacio en disco",
        P(&[Productividad, Sistema]),
    ),
    (
        "busca el contrato en documentos, léelo y crea un recordatorio para renovarlo",
        P(&[Archivos, Productividad]),
    ),
    (
        "pon una canción de jazz y abre la carpeta de proyectos",
        P(&[Productividad, Archivos]),
    ),
    (
        "revisa los tests que fallan, arréglalos y crea una tarea si queda algo pendiente",
        P(&[Programador, Productividad]),
    ),
    ("Necesito que investigues qué es la inflación y que me prepares un informe", S),
    ("lee los PDFs de la carpeta Facturas y hazme un informe con el total", S),
    ("revisa el rendimiento y hazme un reporte", S),
    ("busca la foto del perro, ábrela y pon música", P(&[Archivos, Productividad])),
    (
        "revisa mi código Rust, ejecuta cargo test y apunta las pruebas que fallen como tareas",
        P(&[Programador, Productividad]),
    ),
    (
        "investiga sobre Tauri\ncrea una carpeta para probarlo\napunta una tarea para el sábado",
        P(&[Investigador, Archivos, Productividad]),
    ),
    (
        "mira qué hay en mi escritorio y recuérdame ordenarlo el domingo",
        P(&[Archivos, Productividad]),
    ),
    (
        "explícame por qué mi computadora se calienta y busca artículos sobre cómo evitarlo",
        P(&[Sistema, Investigador]),
    ),
    (
        "abre Spotify, pon mi playlist y revisa cuánta RAM consume",
        P(&[Productividad, Sistema]),
    ),
    (
        "busca todas las imágenes del escritorio y muévelas a una carpeta llamada Fotos, luego crea una tarea para respaldarlas",
        P(&[Archivos, Productividad]),
    ),
    (
        "revisa los errores de mi código, búscalos en internet y apúntalos en mis tareas",
        P(&[Programador, Investigador, Productividad]),
    ),
    ("dime cuánta batería queda y apaga la música", P(&[Sistema, Productividad])),
    (
        "investiga el precio del dólar hoy y anótalo en una tarea",
        P(&[Investigador, Productividad]),
    ),
    ("lee el archivo README del proyecto y explícame cómo instalarlo", S),
    (
        "revisa el uso de disco, busca los archivos más grandes y crea una tarea para borrarlos",
        P(&[Sistema, Archivos, Productividad]),
    ),
    (
        "Primero revisa mis tareas de hoy, después investiga sobre el examen de historia",
        P(&[Productividad, Investigador]),
    ),
    (
        "organiza la carpeta de fotos y investiga cómo hacer copias de seguridad",
        P(&[Archivos, Investigador]),
    ),
    ("crea un archivo con mis tareas pendientes en el escritorio", S),
    (
        "investiga quién ganó el último mundial y dime cuánta memoria uso",
        P(&[Investigador, Sistema]),
    ),
    (
        "revisa el código de la API, investiga el error 500 en internet, crea una tarea y prepara un informe",
        P(&[Programador, Investigador, Productividad, INF]),
    ),
    (
        "Por favor, organice mis documentos y prográmeme una tarea para revisarlos",
        P(&[Archivos, Productividad]),
    ),
    (
        "ahorita revisa por qué se traba la compu y apúntame una tarea",
        P(&[Sistema, Productividad]),
    ),
    ("busca un tutorial de Rust en internet y ábrelo", S),
    (
        "lee el código de main.py y crea una carpeta de respaldo con una copia",
        P(&[Programador, Archivos]),
    ),
    ("abre el administrador de tareas y revisa el consumo de memoria", S),
    (
        "escribe en un archivo las ideas para mi proyecto de tesis y agenda una reunión con mi asesor",
        P(&[Archivos, Productividad]),
    ),
    (
        "diagnostica la memoria, busca qué es el archivo de paginación y crea un informe",
        P(&[Sistema, Investigador, INF]),
    ),
    ("pon música, crea una tarea para lavar ropa y abre la calculadora", S),
    (
        "revisa el código, revisa el disco y revisa mis tareas",
        P(&[Programador, Sistema, Productividad]),
    ),
    (
        "investiga la vida de Frida Kahlo, guarda un resumen en un archivo y crea una tarea para leerlo",
        P(&[Investigador, Archivos, Productividad]),
    ),
    ("mira mis archivos recientes y pon una canción", P(&[Archivos, Productividad])),
    (
        "¿Me ayudas a revisar mi código de Python y después a crear tareas para lo que falte?",
        P(&[Programador, Productividad]),
    ),
    (
        "investiga qué es el phishing y crea una carpeta con lo que encuentres",
        P(&[Investigador, Archivos]),
    ),
    (
        "busca mis apuntes de cálculo y recuérdame estudiarlos mañana",
        P(&[Archivos, Productividad]),
    ),
    (
        "revisa la batería; si está baja, baja el brillo y crea un recordatorio",
        P(&[Sistema, Productividad]),
    ),
    (
        "Canela: 1) organiza mis descargas 2) revisa la RAM 3) dime qué tareas tengo",
        P(&[Archivos, Sistema, Productividad]),
    ),
    ("lee la página de noticias https://ejemplo.com/hoy y hazme un resumen", S),
    ("crea una tarea para mañana y no la borres", S),
    (
        "revisa el código del proyecto pero no lo cambies, y crea tareas con los problemas",
        P(&[Programador, Productividad]),
    ),
    ("borra la tarea 3 y revisa el disco", P(&[Productividad, Sistema])),
    (
        "elimina los archivos temporales y después revisa cuánta memoria liberé",
        P(&[Archivos, Sistema]),
    ),
    ("investiga, revisa y organiza todo", S),
    ("haz lo que te pedí ayer y además pon música", S),
    (
        "abre el proyecto en VS Code, ejecuta los tests y si fallan apúntalo en tareas",
        P(&[Programador, Productividad]),
    ),
    ("investiga la capital de Australia", S),
    (
        "dime qué hay en la carpeta Proyectos y explícame el código del archivo main.rs",
        P(&[Archivos, Programador]),
    ),
    ("crea una tarea, luego otra y luego otra más", S),
    (
        "mueve los PDFs de descargas a documentos y crea una tarea para leerlos",
        P(&[Archivos, Productividad]),
    ),
    ("revisa mi disco y no borres nada", S),
    ("limpia la carpeta de descargas y pon música", P(&[Archivos, Productividad])),
];

fn outcome(a: &Analysis) -> String {
    match a {
        Analysis::Single => "un agente".into(),
        Analysis::ModelPlan => "consultar al modelo".into(),
        Analysis::Clarify(_) => "aclaración".into(),
        Analysis::Plan(p) => format!("plan {:?}", p.steps.iter().map(|s| s.agent).collect::<Vec<_>>()),
    }
}

#[test]
fn planner_evaluation_on_new_requests() {
    assert!(SET.len() >= 100, "{}", SET.len());
    let (mut ok, mut model, mut false_plans, mut extra_calls, mut missed_clarify, mut wrong_plan) = (0, 0, 0, 0, 0, 0);
    let (mut np, mut ns, mut nc) = (0, 0, 0);
    let mut fails = vec![];
    for (text, expected) in SET {
        let got = analyze(text);
        let good = match (expected, &got) {
            (P(agents), Analysis::Plan(p)) => p.steps.iter().map(|s| s.agent).collect::<Vec<_>>() == *agents,
            (S, Analysis::Single) | (C, Analysis::Clarify(_)) => true,
            _ => false,
        };
        match expected {
            P(_) => np += 1,
            S => ns += 1,
            C => nc += 1,
        }
        if good {
            ok += 1;
            continue;
        }
        match (expected, &got) {
            (P(_), Analysis::ModelPlan) => model += 1,
            (P(_), Analysis::Plan(_)) => wrong_plan += 1,
            (S, Analysis::Plan(_)) => false_plans += 1,
            (S, Analysis::ModelPlan) => extra_calls += 1,
            (C, _) => missed_clarify += 1,
            _ => {}
        }
        fails.push(format!("  {text:?}\n      esperado {expected:?} → obtenido {}", outcome(&got)));
    }
    let n = SET.len();
    eprintln!(
        "Evaluación del planificador: {n} solicitudes nuevas ({np} de varios agentes, {ns} de un agente, {nc} que requieren aclaración)"
    );
    eprintln!("  Aciertos de las reglas: {ok}/{n} = {:.0} %", ok as f64 * 100.0 / n as f64);
    eprintln!("  Planes de varios agentes que las reglas no resolvieron pero pasarían al modelo (con validación): {model}");
    eprintln!("  Planes con agentes distintos de los esperados: {wrong_plan}");
    eprintln!("  Planes innecesarios (bastaba un agente; se pide permiso igual): {false_plans}");
    eprintln!("  Consultas extra al modelo innecesarias: {extra_calls}");
    eprintln!("  Aclaraciones que faltaron: {missed_clarify}");
    eprintln!("Fallos:\n{}", fails.join("\n"));
    // Umbral de regresión (no objetivo): el resultado real se documenta.
    assert!(ok * 100 / n >= 60, "{ok}/{n}");
}
