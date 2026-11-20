//! Conjuntos de evaluación del clasificador del enrutador (sólo pruebas).
//!
//! - Conjunto A (100 mensajes): escrito ANTES de mejorar las reglas en la
//!   v0.6.1. Sirvió para medir la línea base y para orientar las mejoras,
//!   así que ya no es una medida independiente.
//! - Conjunto B (60 mensajes): escrito DESPUÉS de las mejoras y no usado para
//!   ajustarlas. Es la medida honesta.
//!
//! Etiquetas: Simple = acción o pregunta trivial; Medium = respuesta normal
//! (explicar, redactar, código corto); Complex = razonamiento largo,
//! análisis, diseño, depuración o varias tareas encadenadas.

use super::router::{complexity, Complexity, Request};
use Complexity::{Complex as C, Medium as M, Simple as S};

pub const SET_A: &[(&str, Complexity)] = &[
    // --- Sencillas (35)
    ("hola", S),
    ("buenas tardes canela", S),
    ("¿qué día es hoy?", S),
    ("abre spotify", S),
    ("abre visual studio code", S),
    ("cierra la sesión de música", S),
    ("pausa", S),
    ("siguiente canción", S),
    ("sube el volumen al máximo", S),
    ("silencia el audio", S),
    ("recuérdame tomar agua en una hora", S),
    ("crea una tarea: comprar pan", S),
    ("agrega a pendientes llamar al dentista", S),
    ("marca como hecha la tarea de matemáticas", S),
    ("¿qué tengo para mañana?", S),
    ("muéstrame mis tareas", S),
    ("busca el archivo presupuesto.xlsx", S),
    ("abre mis descargas", S),
    ("¿cuánto espacio libre tengo?", S),
    ("¿cuánta RAM estoy usando?", S),
    ("gracias canela", S),
    ("buen trabajo", S),
    ("cuéntame un chiste de perros", S),
    ("¿cómo te llamas?", S),
    ("¿cuánto es 15 por 4?", S),
    ("¿cuál es la capital de Japón?", S),
    ("¿quién escribió Don Quijote?", S),
    ("traduce 'gato' al inglés", S),
    ("¿qué hora es en Madrid?", S),
    ("recuerda que mi color favorito es el violeta", S),
    ("olvida lo de mi color favorito", S),
    ("toma una captura de pantalla", S),
    ("abre youtube", S),
    ("pon la tarea de inglés para el viernes", S),
    ("¿estás ahí?", S),
    // --- Normales (35)
    ("explícame qué es una API REST", M),
    ("¿qué es la inflación y por qué sube?", M),
    ("escribe un correo para pedir vacaciones a mi jefe", M),
    ("redacta una disculpa por llegar tarde a la reunión", M),
    ("dame cinco ideas para un regalo de aniversario", M),
    ("resume la historia de la Segunda Guerra Mundial en un párrafo", M),
    ("escribe una función en JavaScript que sume un arreglo", M),
    ("¿cómo creo un entorno virtual en Python?", M),
    ("¿cómo configuro git por primera vez?", M),
    ("explica la diferencia entre RAM y almacenamiento", M),
    ("¿por qué el cielo es azul?", M),
    ("escribe un poema sobre el otoño", M),
    ("dame una receta fácil con pollo y arroz", M),
    ("¿qué ejercicios puedo hacer en casa sin equipo?", M),
    ("explícame cómo funciona un préstamo hipotecario", M),
    ("escribe un mensaje de cumpleaños para mi abuela", M),
    ("¿cómo instalo Python en Windows?", M),
    ("¿qué es Docker y para qué sirve?", M),
    ("dame un ejemplo de consulta SQL con JOIN", M),
    ("corrige la ortografía de este texto: ayer fuimos al sine y bimos una pelicula", M),
    ("traduce este párrafo al inglés: Canela es una perrita que vive en el escritorio y ayuda a su dueño", M),
    ("explica qué es la fotosíntesis para un niño", M),
    ("¿cuáles son los beneficios de dormir bien?", M),
    ("escribe una descripción para vender mi bicicleta", M),
    ("¿cómo funciona el protocolo HTTPS?", M),
    ("dame consejos para una entrevista de trabajo", M),
    ("¿qué es una red neuronal?", M),
    ("explícame los tipos de datos en Rust", M),
    ("escribe un script de bash que liste los archivos grandes", M),
    ("¿cómo hago una tabla dinámica en Excel?", M),
    ("resume este texto: el cambio climático afecta las cosechas, sube el nivel del mar y provoca fenómenos extremos cada vez más frecuentes", M),
    ("¿qué diferencia hay entre un virus y una bacteria?", M),
    ("escribe un tuit anunciando mi nuevo proyecto", M),
    ("¿cómo puedo mejorar mi postura al programar?", M),
    ("explica qué hace el comando git rebase", M),
    // --- Complejas (30)
    ("analiza mi proyecto de React y dime qué partes deberían refactorizarse y por qué", C),
    ("diseña la base de datos de una tienda en línea con usuarios, productos, pedidos y pagos, y justifica cada relación", C),
    ("compara PostgreSQL, MongoDB y SQLite para una app de escritorio: rendimiento, mantenimiento y escalabilidad", C),
    ("depura este código, no funciona:\n```rust\nfn main() {\n    let v = vec![1, 2, 3];\n    for i in 0..=v.len() {\n        println!(\"{}\", v[i]);\n    }\n}\n```", C),
    ("mi aplicación de Node se queda sin memoria después de unas horas, ¿cómo investigo la fuga y qué herramientas uso?", C),
    ("escribe un informe completo sobre las ventajas y riesgos de usar IA en hospitales", C),
    ("crea un plan de estudio de 12 semanas para aprender programación desde cero con objetivos semanales", C),
    ("implementa una API REST en Rust con autenticación JWT, base de datos y pruebas", C),
    ("investiga qué GPU me conviene para entrenar modelos de IA con un presupuesto de 600 dólares y compara opciones", C),
    ("revisa este código y encuentra los problemas de seguridad:\n```js\napp.get('/user', (req, res) => {\n  db.query('SELECT * FROM users WHERE id = ' + req.query.id);\n});\n```", C),
    ("optimiza esta consulta SQL que tarda 30 segundos y explica el plan de ejecución", C),
    ("diseña la arquitectura de microservicios para una app de delivery y explica cómo se comunican", C),
    ("evalúa los pros y contras de mudarme a Canadá para trabajar como programador", C),
    ("¿por qué falla mi build de Tauri en Windows? error: linker `link.exe` not found. Revisa la configuración y dime qué instalar paso a paso", C),
    ("analiza los datos de ventas del último año y propón una estrategia para el próximo trimestre", C),
    ("migra este componente de clase de React a hooks y explica cada cambio:\n```jsx\nclass Contador extends React.Component {\n  state = { n: 0 };\n  render() { return <button onClick={() => this.setState({ n: this.state.n + 1 })}>{this.state.n}</button>; }\n}\n```", C),
    ("demuestra que la raíz cuadrada de 2 es irracional", C),
    ("escribe un ensayo de 1000 palabras sobre la ética de la inteligencia artificial", C),
    ("compara tres frameworks de pruebas para JavaScript y recomiéndame uno para mi proyecto con argumentos", C),
    ("diseña un algoritmo que reparta tareas entre empleados según su disponibilidad y habilidades, y analiza su complejidad", C),
    ("revisa mi currículum y reescríbelo para un puesto de desarrollador senior, explicando cada mejora", C),
    ("haz un análisis financiero de comprar vs. rentar una casa a 20 años con estos datos: precio 2 millones, renta 15 mil, interés 10 %", C),
    ("explica en detalle cómo funciona el recolector de basura de Java y compara G1 con ZGC", C),
    ("refactoriza mi función de 200 líneas en funciones pequeñas con pruebas unitarias", C),
    ("investiga y resume las diferencias legales entre una SA y una SRL en México para emprender", C),
    ("tengo este error en producción: TypeError: Cannot read properties of undefined (reading 'map'). Analiza las causas posibles y cómo prevenirlo", C),
    ("planifica la migración de mi web de WordPress a Next.js sin perder posicionamiento", C),
    ("diseña una estrategia de copias de seguridad 3-2-1 para mi PC y mis proyectos, con herramientas concretas", C),
    ("analiza este contrato de alquiler y señala las cláusulas abusivas", C),
    ("crea un modelo de negocio completo para una app de mascotas: clientes, ingresos, costos y competencia", C),
];

pub const SET_B: &[(&str, Complexity)] = &[
    // Escrito después de las mejoras de la v0.6.1 (no usado para ajustar).
    // --- Sencillas (20)
    ("buenas noches", S),
    ("abre el bloc de notas", S),
    ("¿qué tiempo hace?", S),
    ("pon música", S),
    ("baja el brillo", S),
    ("crea una tarea para revisar el correo a las 9", S),
    ("¿tengo algo pendiente esta semana?", S),
    ("recuérdame el cumpleaños de Luis el 3 de mayo", S),
    ("abre la carpeta documentos", S),
    ("¿cuántos núcleos tiene mi procesador?", S),
    ("¿cuánto es 7 al cuadrado?", S),
    ("¿en qué año llegó el hombre a la Luna?", S),
    ("dime un dato curioso", S),
    ("muestra mis archivos recientes", S),
    ("vuelve a poner la canción anterior", S),
    ("eres muy útil, gracias", S),
    ("borra la tarea de comprar pan", S),
    ("¿cómo se dice perro en francés?", S),
    ("libera la memoria", S),
    ("¿sigues escuchando?", S),
    // --- Normales (20)
    ("explícame qué es el interés compuesto con un ejemplo", M),
    ("escribe una carta de recomendación para un compañero de trabajo", M),
    ("¿qué es TypeScript y por qué usarlo?", M),
    ("dame una rutina de 20 minutos para estirar", M),
    ("¿cómo funciona una VPN?", M),
    ("escribe una función en Python que valide un correo", M),
    ("resume en cinco puntos cómo ahorrar dinero", M),
    ("¿cómo cambio la contraseña del wifi de mi router?", M),
    ("explica qué son los hooks de React", M),
    ("redacta una publicación de LinkedIn sobre mi nuevo trabajo", M),
    ("¿qué diferencia hay entre HTTP y WebSocket?", M),
    ("dame tres recetas veganas para la semana", M),
    ("¿por qué se calienta mi laptop al jugar?", M),
    ("escribe un ejemplo de async/await en JavaScript", M),
    ("¿cómo organizo mejor mi tiempo para estudiar?", M),
    ("explícame cómo se calcula el IVA", M),
    ("corrige este párrafo: los niños jugavan en el parque asta tarde", M),
    ("¿qué es Kubernetes en pocas palabras?", M),
    ("escribe un cuento corto para dormir sobre una perrita", M),
    ("¿cómo funciona la memoria virtual en un sistema operativo?", M),
    // --- Complejas (20)
    ("analiza por qué mi PC se pone lenta al abrir Chrome y Ollama a la vez y propón soluciones ordenadas por impacto", C),
    ("diseña el esquema de una app de finanzas personales con presupuestos, categorías y sincronización entre dispositivos", C),
    ("compara AWS, Azure y Google Cloud para alojar una API pequeña: costos, facilidad y servicios", C),
    ("revisa este código Python y dime por qué es lento:\n```python\nresultado = []\nfor x in datos:\n    if x not in resultado:\n        resultado.append(x)\n```", C),
    ("escribe un plan detallado para lanzar mi canal de YouTube de programación en tres meses", C),
    ("implementa un sistema de caché LRU en Rust con pruebas y explica las decisiones", C),
    ("investiga las mejores prácticas de seguridad para una app Tauri y aplícalas a mi proyecto", C),
    ("evalúa si me conviene aprender Go o Rust para backend considerando mercado laboral y curva de aprendizaje", C),
    ("depura este error de compilación: error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable, y explica la causa", C),
    ("diseña un sistema de recomendaciones de películas y explica el algoritmo paso a paso", C),
    ("redacta un informe de incidente con causas, cronología y acciones correctivas para una caída del servidor de 2 horas", C),
    ("analiza estos gastos mensuales y propón un presupuesto: renta 8000, comida 4000, transporte 1500, ocio 3000, ingresos 18000", C),
    ("migra mi base de datos de MySQL a PostgreSQL sin tiempo de inactividad y detalla los riesgos", C),
    ("compara los modelos Qwen3, Llama 3 y Mistral para usar en local con 16 GB de VRAM", C),
    ("crea una arquitectura para que Canela tenga agentes especializados y explica cómo compartirían contexto", C),
    ("escribe pruebas unitarias completas para un módulo de autenticación con casos límite", C),
    ("analiza la complejidad temporal de quicksort y mergesort y cuándo usar cada uno", C),
    ("planifica la mudanza de mi oficina con presupuesto, cronograma y lista de tareas por semana", C),
    ("¿por qué mi consulta tarda tanto? EXPLAIN muestra Seq Scan en una tabla de 10 millones de filas; propón índices y reescribe la consulta", C),
    ("diseña un plan de pruebas para validar Canela en Windows 11 antes de publicar una versión estable", C),
];

pub fn evaluate(set: &[(&str, Complexity)]) -> (usize, usize, Vec<String>, [[usize; 3]; 3]) {
    let idx = |c: Complexity| match c {
        S => 0,
        M => 1,
        C => 2,
    };
    let mut ok = 0;
    let mut misses = Vec::new();
    let mut matrix = [[0usize; 3]; 3];
    for (text, expected) in set {
        let (got, _) = complexity(&Request {
            text: (*text).into(),
            ..Default::default()
        });
        matrix[idx(*expected)][idx(got)] += 1;
        if got == *expected {
            ok += 1;
        } else {
            misses.push(format!("{text:?}: esperado {expected:?}, obtenido {got:?}"));
        }
    }
    (ok, set.len(), misses, matrix)
}

pub fn print(name: &str, r: &(usize, usize, Vec<String>, [[usize; 3]; 3])) {
    let (ok, n, misses, m) = r;
    eprintln!("{name}: {ok}/{n} = {:.0} %", *ok as f64 * 100.0 / *n as f64);
    eprintln!("  matriz (filas = esperado S/M/C; columnas = obtenido S/M/C): {m:?}");
    for x in misses {
        eprintln!("  fallo: {x}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_a_measure() {
        let r = evaluate(SET_A);
        print("Conjunto A (100)", &r);
        // Nunca proponer gastar por algo sencillo.
        assert_eq!(r.3[0][2], 0, "sencillas clasificadas como complejas");
    }

    #[test]
    fn set_b_measure() {
        let r = evaluate(SET_B);
        print("Conjunto B (60, independiente)", &r);
        assert_eq!(r.3[0][2], 0, "sencillas clasificadas como complejas");
    }
}
