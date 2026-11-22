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

/// Conjunto C (v0.7.3): 110 solicitudes NUEVAS, escritas después de terminar
/// las mejoras generales del planificador (que se guiaron por los fallos del
/// conjunto B) y medidas una sola vez.
const SET_C: &[(&str, E)] = &[
    (
        "Oye Canela, échale un ojo a mi repositorio de Node y luego agéndame una revisión para el jueves",
        P(&[Programador, Productividad]),
    ),
    (
        "necesito que averigües el horario del museo y que lo apuntes en mis pendientes",
        P(&[Investigador, Productividad]),
    ),
    (
        "Busca en Google cómo configurar un VPN y apúntame los pasos en una nota",
        P(&[Investigador, Productividad]),
    ),
    (
        "mira la temperatura de la GPU y, si pasa de 80 grados, ponme una alarma",
        P(&[Sistema, Productividad]),
    ),
    (
        "1) actualiza el README del proyecto 2) crea una carpeta 'docs' 3) recuérdame publicar el viernes",
        P(&[Programador, Archivos, Productividad]),
    ),
    (
        "Lee el Word de la propuesta y hazme una lista de pendientes con lo que falte",
        P(&[Archivos, Productividad]),
    ),
    (
        "investiga las ventajas de Linux y guárdalas en un documento en Documentos",
        P(&[Investigador, Archivos]),
    ),
    ("¿cuánta memoria libre hay? además pon algo de música", P(&[Sistema, Productividad])),
    (
        "corrige el error de sintaxis en index.js y luego busca en la web por qué pasa",
        P(&[Programador, Investigador]),
    ),
    (
        "pásame a una carpeta nueva todas las capturas del escritorio y recuérdame borrarlas en un mes",
        P(&[Archivos, Productividad]),
    ),
    (
        "dime qué es un SSD, revisa cuánto espacio queda en el mío y anota si debo cambiarlo",
        P(&[Investigador, Sistema, Productividad]),
    ),
    (
        "escanea mi carpeta de proyectos, encuentra los scripts de Python viejos y propón cuáles borrar",
        P(&[Archivos, Programador]),
    ),
    (
        "abre VS Code con mi proyecto y pon el volumen al 30%",
        P(&[Programador, Productividad]),
    ),
    (
        "Busca la factura de la luz en descargas, léela y agenda el pago",
        P(&[Archivos, Productividad]),
    ),
    (
        "Investiga quién escribió Cien años de soledad y crea una tarea para comprar el libro",
        P(&[Investigador, Productividad]),
    ),
    ("Revisa el rendimiento del equipo y después compárteme un resumen", S),
    (
        "analiza los logs de la app, identifica las excepciones y crea tickets como tareas",
        P(&[Programador, Productividad]),
    ),
    ("mira cuánto ocupa mi carpeta de descargas y organízala por tipo", S),
    (
        "busca qué significa el error 0x80070005, revisa los permisos de la carpeta y anota la solución",
        P(&[Investigador, Archivos, Productividad]),
    ),
    ("programa un recordatorio para tomar agua cada hora y pon música lo-fi", S),
    (
        "Lee el CSV de gastos y dime en qué categoría gasto más, luego guarda el análisis en un archivo",
        S,
    ),
    ("Investiga el clima de Monterrey y dime si necesito llevar paraguas", S),
    ("revisa mis tareas pendientes de hoy y abre la calculadora", S),
    (
        "dime cuánta RAM usa Chrome y busca en internet cómo reducirlo",
        P(&[Sistema, Investigador]),
    ),
    (
        "haz un respaldo de mi carpeta de tesis y recuérdame hacerlo cada viernes",
        P(&[Archivos, Productividad]),
    ),
    ("Mira si hay actualizaciones del sistema", S),
    (
        "explícame este error de TypeScript y busca un artículo que lo explique mejor",
        P(&[Programador, Investigador]),
    ),
    (
        "revisa el código del login, escribe pruebas y apunta en mis tareas lo que falle",
        P(&[Programador, Productividad]),
    ),
    (
        "busca el PDF del manual del router y explícame cómo cambiar la contraseña del wifi",
        S,
    ),
    (
        "organiza mis fotos por año y luego investiga qué app sirve para editarlas",
        P(&[Archivos, Investigador]),
    ),
    ("dime la hora y la fecha", S),
    ("Canela, ¿qué tal tu día?", S),
    ("crea la carpeta 'Viajes' y dentro un archivo con la lista de equipaje", S),
    ("investiga el significado de 'resiliencia' y agrégalo a mis recuerdos", S),
    (
        "revisa si el disco está lleno y, en caso de que sí, busca archivos grandes en descargas",
        P(&[Sistema, Archivos]),
    ),
    ("mándale un correo a mi jefe", S),
    (
        "busca 'tutorial de Docker' en YouTube y crea una tarea para verlo",
        P(&[Investigador, Productividad]),
    ),
    ("Lee mi CV, mejóralo y guárdalo como CV_final en el escritorio", S),
    (
        "pon una rutina de estudio para las tardes y revisa que mi PC no se caliente cuando juego",
        P(&[Productividad, Sistema]),
    ),
    (
        "mira qué apps arrancan con Windows y crea una tarea para desactivar las innecesarias",
        P(&[Sistema, Productividad]),
    ),
    (
        "compara el precio del dólar de hoy con el de ayer y anótalo",
        P(&[Investigador, Productividad]),
    ),
    ("revisa mi código de C# y explícame qué hace la clase Usuario", S),
    ("ordena los archivos de mi escritorio y vacía la papelera", S),
    (
        "busca noticias sobre la Fórmula 1 y ponme música de fondo",
        P(&[Investigador, Productividad]),
    ),
    (
        "Investiga cómo funciona HTTPS y luego revisa si mi servidor de Express lo usa bien",
        P(&[Investigador, Programador]),
    ),
    (
        "crea una tarea para el dentista el martes, otra para el gimnasio y otra para estudiar",
        S,
    ),
    ("revisa el uso de la CPU, la RAM y el disco", S),
    ("dime qué tengo en Descargas y borra los instaladores viejos", S),
    (
        "lee la documentación de la API en https://docs.ejemplo.com y escribe un ejemplo en Python",
        P(&[Investigador, Programador]),
    ),
    (
        "busca recetas veganas fáciles y agenda una para cocinar el sábado",
        P(&[Investigador, Productividad]),
    ),
    (
        "revisa mi proyecto de Unity y luego dime cuánta VRAM tengo libre",
        P(&[Programador, Sistema]),
    ),
    ("toma nota de que el examen es el 12 y avísame dos días antes", S),
    (
        "organiza la carpeta Música por artista y luego reproduce algo de Queen",
        P(&[Archivos, Productividad]),
    ),
    (
        "investiga sobre la Revolución Mexicana, haz un resumen y guárdalo en un archivo llamado historia.txt",
        P(&[Investigador, INF, Archivos]),
    ),
    ("revisa la memoria, revisa el disco y revisa la batería", S),
    ("escribe un script de Bash que respalde mi carpeta de fotos", S),
    (
        "busca mis notas de la clase de química y crea tareas para los temas del examen",
        P(&[Archivos, Productividad]),
    ),
    ("cierra la música y dime cómo va la memoria", P(&[Productividad, Sistema])),
    ("busca en Wikipedia qué es la fotosíntesis y explícamelo como para un niño", S),
    ("abre la carpeta del proyecto y ejecuta las pruebas", S),
    ("recuérdame mañana a las 8 llamar al doctor y apaga el volumen", S),
    (
        "investiga qué laptop me conviene para programar y anota las 3 mejores opciones",
        P(&[Investigador, Productividad]),
    ),
    ("revisa los commits de esta semana y hazme un reporte", S),
    (
        "dime cuánto espacio libre tengo y, si hay menos de 10 GB, organiza mis descargas",
        P(&[Sistema, Archivos]),
    ),
    (
        "Busca el contrato de renta en mis documentos y recuérdame la fecha de vencimiento",
        P(&[Archivos, Productividad]),
    ),
    (
        "crea una carpeta de backup, copia ahí mi proyecto y anota en tareas que ya está hecho",
        P(&[Archivos, Productividad]),
    ),
    ("explícame qué es Kubernetes y si me sirve para mi proyecto de Django", S),
    (
        "investiga los requisitos de la visa, guarda la lista en un archivo y agenda la cita en el consulado",
        P(&[Investigador, Archivos, Productividad]),
    ),
    ("lee el archivo package.json y dime qué dependencias están desactualizadas", S),
    (
        "chécame si la compu se está calentando y pon una alarma para descansar en una hora",
        P(&[Sistema, Productividad]),
    ),
    (
        "busca en la web el horario del banco y ponme un recordatorio",
        P(&[Investigador, Productividad]),
    ),
    ("organízame el escritorio y dime cuánta batería me queda", P(&[Archivos, Sistema])),
    (
        "elimina las tareas completadas y luego revisa cuánta memoria usa Canela",
        P(&[Productividad, Sistema]),
    ),
    ("revisa el disco duro y bórrame todo lo que no sirva", C),
    ("borra eso que te dije y abre la calculadora", C),
    ("mueve todo a otra parte y crea una tarea", C),
    ("crea una tarea para comprar leche y luego no la crees", C),
    ("pon música, investiga el clima y no pongas música", C),
    ("organiza mis descargas pero no organices nada, y revisa la memoria", C),
    ("limpia aquello y revisa la batería", C),
    ("vacía la carpeta y después investiga qué es un SSD", C),
    ("investiga sobre Python y no investigues nada", C),
    (
        "revisa el script de backup en Python y apunta como tarea cualquier error que encuentres",
        P(&[Programador, Productividad]),
    ),
    (
        "investiga qué causa el pantallazo azul y revisa si mi RAM está bien",
        P(&[Investigador, Sistema]),
    ),
    (
        "Busca información sobre la beca Conacyt y crea una carpeta para los documentos de la solicitud",
        P(&[Investigador, Archivos]),
    ),
    (
        "mira las imágenes de la carpeta Capturas y crea una tarea para revisarlas",
        P(&[Archivos, Productividad]),
    ),
    ("revisa tu memoria de lo que te dije y crea una tarea", S),
    (
        "baja el volumen y revisa qué proceso consume tanta CPU",
        P(&[Productividad, Sistema]),
    ),
    (
        "investiga cómo se hace un README bueno y luego mejora el de mi proyecto",
        P(&[Investigador, Programador]),
    ),
    (
        "dime qué hay en mi carpeta de Música y pon la primera canción",
        P(&[Archivos, Productividad]),
    ),
    (
        "revisa por qué tarda tanto en arrancar la PC y busca soluciones en internet",
        P(&[Sistema, Investigador]),
    ),
    ("crea una carpeta para cada materia del semestre", S),
    (
        "investiga la mejor forma de aprender inglés y crea una rutina diaria para practicarlo",
        P(&[Investigador, Productividad]),
    ),
    (
        "revisa mi código y, si encuentras algo raro, crea una tarea",
        P(&[Programador, Productividad]),
    ),
    ("dime el uso de GPU y apunta el valor en una nota", P(&[Sistema, Productividad])),
    ("lee el PDF del temario y crea una carpeta por cada tema", S),
    ("busca la definición de algoritmo y guárdala en mis recuerdos", S),
    (
        "Primero organiza mis descargas. Segundo, revisa la batería. Tercero, pon música.",
        P(&[Archivos, Sistema, Productividad]),
    ),
    ("investiga quién inventó la imprenta", S),
    (
        "revisa la configuración de Git de mi proyecto y anota lo que haya que cambiar",
        P(&[Programador, Productividad]),
    ),
    (
        "dime cuánto pesa la carpeta de videos y busca cómo comprimir videos",
        P(&[Archivos, Investigador]),
    ),
    ("sube el brillo y pon música relajante", P(&[Sistema, Productividad])),
    (
        "abre el documento del presupuesto y crea una tarea para revisarlo con mi pareja",
        P(&[Archivos, Productividad]),
    ),
    (
        "investiga las causas de la inflación, escribe un resumen en un archivo y recuérdame leerlo",
        P(&[Investigador, Archivos, Productividad]),
    ),
    ("revisa si tengo tareas atrasadas y organiza mi semana", S),
    (
        "haz una copia de mi carpeta de proyectos en el disco externo y avísame al terminar",
        S,
    ),
    ("busca qué es un NAS y dime si me conviene comprar uno", S),
    ("revisa los errores de compilación y explícame cada uno", S),
    (
        "investiga sobre ciberseguridad, crea una carpeta con lo que encuentres y prepárame un informe",
        P(&[Investigador, Archivos, INF]),
    ),
    (
        "mira qué programas consumen más memoria y crea una tarea para desinstalar los que no uso",
        P(&[Sistema, Productividad]),
    ),
];

fn outcome(a: &Analysis) -> String {
    match a {
        Analysis::Single => "un agente".into(),
        Analysis::ModelPlan => "consultar al modelo".into(),
        Analysis::Clarify(_) => "aclaración".into(),
        Analysis::Plan(p) => format!("plan {:?}", p.steps.iter().map(|s| s.agent).collect::<Vec<_>>()),
    }
}

fn evaluate(name: &str, set: &[(&str, E)]) -> usize {
    let (mut ok, mut model, mut false_plans, mut extra_calls, mut missed_clarify, mut wrong_plan) = (0, 0, 0, 0, 0, 0);
    let (mut np, mut ns, mut nc) = (0, 0, 0);
    let mut fails = vec![];
    for (text, expected) in set {
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
    let n = set.len();
    eprintln!("{name}: {n} solicitudes ({np} de varios agentes, {ns} de un agente, {nc} que requieren aclaración)");
    eprintln!("  Aciertos de las reglas: {ok}/{n} = {:.0} %", ok as f64 * 100.0 / n as f64);
    eprintln!("  Planes de varios agentes que las reglas no resolvieron pero pasarían al modelo (con validación): {model}");
    eprintln!("  Planes con agentes distintos de los esperados: {wrong_plan}");
    eprintln!("  Planes innecesarios (bastaba un agente; se pide permiso igual): {false_plans}");
    eprintln!("  Consultas extra al modelo innecesarias: {extra_calls}");
    eprintln!("  Aclaraciones que faltaron: {missed_clarify}");
    eprintln!("Fallos:\n{}", fails.join("\n"));
    ok
}

/// Conjunto B: se midió a ciegas en la v0.7.2 (83 %) y después guió las
/// mejoras de la v0.7.3, así que ya es de desarrollo (resultado optimista).
#[test]
fn planner_evaluation_on_new_requests() {
    assert!(SET.len() >= 100, "{}", SET.len());
    let ok = evaluate("Conjunto B (desarrollo)", SET);
    assert!(ok * 100 / SET.len() >= 60, "{ok}/{}", SET.len());
}

/// Conjunto C: la medición nueva de la v0.7.3.
#[test]
fn planner_evaluation_set_c() {
    assert!(SET_C.len() >= 100, "{}", SET_C.len());
    let ok = evaluate("Conjunto C (nuevo)", SET_C);
    // Umbral de regresión (no objetivo): el resultado real se documenta.
    assert!(ok * 100 / SET_C.len() >= 60, "{ok}/{}", SET_C.len());
}

/// v0.8: ¿cuántas solicitudes reales generan objetivos comprobables? Se mide
/// con los conjuntos B y C (escritos para el planificador, no para el
/// verificador). Las de aclaración no cuentan (no se ejecutan).
#[test]
fn verifier_coverage_on_planner_sets() {
    let mut with_goals = 0;
    let mut total = 0;
    let mut goals = 0;
    let mut missing = vec![];
    for (text, expected) in SET.iter().chain(SET_C.iter()) {
        if matches!(expected, C) {
            continue;
        }
        total += 1;
        let e = crate::verification::expectations(text);
        goals += e.len();
        if e.is_empty() {
            missing.push(format!("  {text:?}"));
        } else {
            with_goals += 1;
        }
    }
    eprintln!(
        "Cobertura del verificador: {with_goals}/{total} solicitudes ({:.0} %) con al menos un objetivo comprobable; {goals} objetivos en total",
        with_goals as f64 * 100.0 / total as f64
    );
    eprintln!("Sin objetivos comprobables:\n{}", missing.join("\n"));
    assert!(with_goals * 100 / total >= 50);
}
