//! Evaluación del verificador (cierre de la v0.8).
//!
//! 45 escenarios NUEVOS escritos ANTES de mejorar el verificador y
//! etiquetados a mano con el resultado ideal según las categorías pedidas:
//! - `completo`: hecho y verificado con evidencia;
//! - `parcial`: una parte sí y otra no;
//! - `no_logrado`: nada de lo pedido se hizo;
//! - `no_verificable`: se hizo (o se respondió), pero la evidencia disponible
//!   no permite confirmarlo;
//! - `sin_comprobar`: charla, no había acción que comprobar.
//!
//! Las etiquetas no se cambian después de medir. Se mide antes y después de
//! las mejoras (el «después» no es a ciegas: lo escribió el mismo equipo que
//! hizo el verificador).

use crate::agents::ActionRecord;
use crate::verification::assess;

fn a(tool: &str, status: &str, verified: Option<bool>, label: &str, detail: &str, progress: Option<[usize; 2]>) -> ActionRecord {
    ActionRecord {
        tool: tool.into(),
        label: label.into(),
        status: status.into(),
        verified,
        detail: Some(detail.into()),
        progress,
    }
}

const WIKI: &str = "Fuente: https://es.wikipedia.org/wiki/Teckel\nTítulo: Teckel\nEl teckel es una raza de perro de origen alemán, de patas cortas y cuerpo alargado. Se crió para cazar tejones. Su peso ronda los 9 kilogramos.";

#[allow(clippy::type_complexity)]
fn scenarios() -> Vec<(&'static str, Vec<ActionRecord>, &'static str, &'static str, &'static str)> {
    let ok = |t: &str, v: Option<bool>, l: &str| a(t, "ok", v, l, "", None);
    vec![
        // Acciones cuyo efecto no se puede comprobar.
        ("abre la calculadora", vec![ok("abrir_aplicacion", None, "Abrir calculadora")], "Abrí la calculadora.", "", "no_verificable"),
        ("pon música", vec![ok("controlar_multimedia", None, "Reproducir")], "", "", "no_verificable"),
        (
            "abre la calculadora y crea una tarea para pagar la luz",
            vec![ok("abrir_aplicacion", None, "Abrir calculadora"), ok("crear_tarea", Some(true), "Crear tarea: Pagar la luz")],
            "",
            "",
            "no_verificable",
        ),
        // Consultas: el registro de la consulta ES la evidencia.
        ("¿cuánta RAM uso?", vec![ok("info_sistema", None, "Estado del equipo")], "Usas 9 GB.", "", "completo"),
        ("lee el archivo notas.txt", vec![ok("leer_archivo", None, "Leer «notas.txt»")], "Dice: comprar pan.", "", "completo"),
        ("revisa mi proyecto web", vec![ok("analizar_proyecto", None, "Analizar proyecto")], "Es un proyecto React.", "", "completo"),
        ("busca el PDF del contrato", vec![ok("buscar_archivos", None, "Buscar «contrato»")], "Está en Documentos.", "", "completo"),
        ("¿qué tareas tengo hoy?", vec![ok("listar_tareas", None, "Revisar tus tareas")], "Tienes 2.", "", "completo"),
        // Responder datos SIN consultar.
        ("¿qué tareas tengo hoy?", vec![], "Tienes 3 tareas para hoy.", "", "no_logrado"),
        ("dime cuánto espacio libre tengo", vec![], "Tienes 120 GB libres.", "", "no_logrado"),
        ("revisa mi proyecto web", vec![], "Lo revisé: todo bien.", "", "no_logrado"),
        // Organizar y automatizaciones con avance medido.
        ("organiza mis descargas", vec![ok("organizar_carpeta", Some(true), "Organizar Descargas")], "", "", "completo"),
        (
            "organiza mis descargas",
            vec![a("organizar_carpeta", "ok", Some(false), "Organizar Descargas", "quedan 3 de 10 archivos sin mover", Some([7, 10]))],
            "Listo, organizado.",
            "",
            "parcial",
        ),
        (
            "ejecuta mi rutina de la mañana",
            vec![a("ejecutar_rutina", "ok", Some(false), "Rutina «mañana»", "2 de 3 pasos completados", Some([2, 3]))],
            "",
            "",
            "parcial",
        ),
        (
            "ejecuta mi rutina de la mañana",
            vec![a("ejecutar_rutina", "ok", Some(true), "Rutina «mañana»", "3 de 3 pasos completados", Some([3, 3]))],
            "",
            "",
            "completo",
        ),
        (
            "ejecuta mi rutina de la mañana",
            vec![a("ejecutar_rutina", "ok", Some(false), "Rutina «mañana»", "0 de 3 pasos completados", Some([0, 3]))],
            "",
            "",
            "no_logrado",
        ),
        // Tareas y recordatorios.
        ("crea una tarea para el dentista con recordatorio el martes a las 9", vec![ok("crear_tarea", Some(true), "Crear tarea: Dentista")], "", "", "completo"),
        (
            "crea una tarea para el dentista con recordatorio el martes a las 9",
            vec![a("crear_tarea", "ok", Some(false), "Crear tarea: Dentista", "la tarea existe, pero el recordatorio NO quedó guardado", None)],
            "Listo.",
            "",
            "no_logrado",
        ),
        ("marca como hecha la tarea 4", vec![ok("actualizar_tarea", Some(true), "Completar: Pagar luz")], "", "", "completo"),
        ("marca como hecha la tarea 4", vec![], "Listo, marcada como hecha.", "", "no_logrado"),
        ("crea 3 tareas para el examen", vec![ok("crear_tarea", Some(true), "a"), ok("crear_tarea", Some(true), "b")], "", "", "parcial"),
        ("recuerda que mi color favorito es el morado", vec![ok("recordar", Some(true), "Recordar")], "", "", "completo"),
        // Investigación: procedencia y respaldo.
        (
            "investiga el teckel",
            vec![ok("buscar_wikipedia", None, "Wikipedia: Teckel")],
            "El teckel es una raza alemana de patas cortas que se crió para cazar tejones.\nFuente: https://es.wikipedia.org/wiki/Teckel",
            WIKI,
            "completo",
        ),
        (
            "investiga el teckel",
            vec![ok("buscar_wikipedia", None, "Wikipedia: Teckel")],
            "El teckel es una raza alemana. Es la raza favorita de la familia real británica desde hace siglos.\nFuente: https://es.wikipedia.org/wiki/Teckel",
            WIKI,
            "no_verificable",
        ),
        (
            "investiga el teckel",
            vec![ok("buscar_wikipedia", None, "Wikipedia: Teckel")],
            "El teckel es una raza alemana que vive unos 16 años.\nFuente: https://es.wikipedia.org/wiki/Teckel",
            WIKI,
            "no_verificable",
        ),
        (
            "investiga el teckel",
            vec![ok("buscar_wikipedia", None, "Wikipedia: Teckel")],
            "El teckel es una raza alemana de patas cortas que se crió para cazar tejones.",
            WIKI,
            "parcial",
        ),
        (
            "investiga el teckel",
            vec![ok("buscar_wikipedia", None, "Wikipedia: Teckel")],
            "El teckel es una raza alemana de patas cortas.\nFuente: https://otra-pagina.example/teckel",
            WIKI,
            "parcial",
        ),
        ("investiga el teckel", vec![], "Según Wikipedia, el teckel es alemán.", "", "no_logrado"),
        ("investiga el teckel", vec![a("buscar_wikipedia", "error", None, "buscar_wikipedia «Teckel»", "No pude consultar Wikipedia: tiempo agotado", None)], "", "", "no_logrado"),
        // Programación: sintaxis y comprobaciones ejecutadas.
        (
            "corrige el error de app.js y comprueba que funciona",
            vec![ok("escribir_archivo", Some(true), "Modificar «app.js»"), ok("comprobar_codigo", Some(true), "Comprobar sintaxis de app.js")],
            "",
            "",
            "completo",
        ),
        (
            "corrige el error de app.js y comprueba que funciona",
            vec![
                ok("escribir_archivo", Some(true), "Modificar «app.js»"),
                a("comprobar_codigo", "ok", Some(false), "Ejecutar pruebas (npm test)", "2 pruebas fallaron (código de salida 1)", None),
            ],
            "Listo.",
            "",
            "parcial",
        ),
        (
            "corrige el error de app.js y comprueba que funciona",
            vec![ok("escribir_archivo", Some(true), "Modificar «app.js»")],
            "Corregido y probado.",
            "",
            "parcial",
        ),
        ("ejecuta las pruebas de mi proyecto", vec![a("comprobar_codigo", "denegada", None, "Ejecutar pruebas (npm test)", "", None)], "", "", "no_logrado"),
        ("ejecuta las pruebas de mi proyecto", vec![ok("comprobar_codigo", Some(true), "Ejecutar pruebas (npm test)")], "", "", "completo"),
        ("ejecuta las pruebas de mi proyecto", vec![], "Todas las pruebas pasan.", "", "no_logrado"),
        (
            "escribe config.json con el modo oscuro",
            vec![a("escribir_archivo", "ok", Some(false), "Crear «config.json»", "posible error de sintaxis: JSON no válido", None)],
            "",
            "",
            "no_logrado",
        ),
        // Archivos.
        ("crea la carpeta Viajes", vec![ok("crear_carpeta", Some(true), "Crear carpeta «Viajes»")], "", "", "completo"),
        ("guarda el resumen en resumen.md", vec![ok("escribir_archivo", Some(true), "Crear «otro.md»")], "Guardado.", "", "no_logrado"),
        ("abre la carpeta de proyectos", vec![ok("abrir_ruta", None, "Abrir Proyectos")], "", "", "no_verificable"),
        ("abre la calculadora", vec![a("abrir_aplicacion", "error", None, "abrir_aplicacion «calculadora»", "No encontré «calculadora».", None)], "", "", "no_logrado"),
        // Mezclas.
        (
            "borra la tarea 3 y pon música",
            vec![ok("eliminar_tarea", Some(true), "Eliminar tarea: X"), ok("controlar_multimedia", None, "Reproducir")],
            "",
            "",
            "no_verificable",
        ),
        (
            "borra la tarea 3 y pon música",
            vec![a("eliminar_tarea", "denegada", None, "Eliminar tarea: X", "", None), ok("controlar_multimedia", None, "Reproducir")],
            "",
            "",
            "parcial",
        ),
        (
            "investiga el teckel y guárdalo en teckel.txt",
            vec![ok("buscar_wikipedia", None, "Wikipedia: Teckel"), ok("escribir_archivo", Some(true), "Crear «teckel.txt»")],
            "Guardé en teckel.txt que es una raza alemana de patas cortas.\nFuente: https://es.wikipedia.org/wiki/Teckel",
            WIKI,
            "completo",
        ),
        // Charla.
        ("hola, ¿cómo estás?", vec![], "¡Guau! Muy bien.", "", "sin_comprobar"),
        ("explícame qué es la recursividad", vec![], "Es cuando una función se llama a sí misma.", "", "sin_comprobar"),
    ]
}

#[test]
fn verifier_closing_evaluation() {
    let cases = scenarios();
    let n = cases.len();
    let mut ok = 0;
    let mut false_complete = 0;
    let mut by_label: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for (req, actions, answer, fetched, expected) in &cases {
        let v = assess(req, actions, answer, fetched);
        let e = by_label.entry(expected).or_default();
        e.1 += 1;
        if v.status == *expected {
            ok += 1;
            e.0 += 1;
        } else {
            if v.status == "completo" {
                false_complete += 1;
            }
            eprintln!("  fallo: {req:?} → {} (esperado {expected})", v.status);
        }
    }
    eprintln!("Verificador (cierre v0.8): {ok}/{n} escenarios; «completo» falsos: {false_complete}");
    for (k, (good, total)) in &by_label {
        eprintln!("  {k}: {good}/{total}");
    }
    assert!(n >= 45, "{n}");
    // Umbral de regresión (no objetivo): el resultado real se documenta.
    assert!(ok * 100 / n >= 30, "{ok}/{n}");
}
