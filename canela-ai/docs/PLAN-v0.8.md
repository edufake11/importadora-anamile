# CANELA AI — v0.7.3 (planificador) y v0.8 (verificación de resultados)

Estado: **rama de desarrollo, no es una versión estable**. Nada de esto se ha probado aún en Windows 11 real. La versión validada sigue siendo la **v0.5**.

---

## Parte 1 — Planificador v0.7.3: mejoras generales

Las 17 solicitudes que fallaban en la v0.7.2 se analizaron por **tipo de fallo**. Para cada tipo se hizo una mejora general, no reglas para esas frases:

| Tipo de fallo | Mejora general |
|---|---|
| Órdenes no reconocidas («apaga», «toma nota», «chécame») | +19 verbos de uso común: apaga, enciende, toma, checa, cierra, activa, desactiva, quita, cambia, inicia, detén, avisa, consigue, obtén, instala, configura, descarga, repasa, escanea (con sus formas irregulares) |
| Temas no reconocidos («Dockerfile», «se calienta», «mis apuntes», «busca en internet», «qué es…») | Vocabulario por temas: términos de desarrollo (docker, servidor, logs, readme, json…), de investigación (en internet, en la web, qué es, cómo funciona, tutorial…), de productividad (anota, apunta, alarma, calendario…), de archivos (documento, apuntes, excel, csv, video, respaldo…) y del sistema (calienta, ventilador, administrador de tareas, paginación, brillo, wifi) |
| Condiciones («y si fallan, apúntalo») | Una parte puede empezar en «si…» cuando le sigue una orden |
| «¿Me ayudas a revisar… y después a crear…?» | «a + infinitivo» tras un conector inicia una parte nueva |
| Referencias a la parte anterior («qué tareas tengo apuntadas **ahí**») | La parte se une a la anterior (la hace el mismo agente) |
| Objeto vago con destino («mueve **eso** a la otra carpeta») | Se mira sólo el objeto (lo que va antes de «a», «en», «hacia»…) → pide aclaración |
| Consultas innecesarias al modelo | Sólo se consulta si una parte que las reglas no pudieron dividir mezcla varios especialistas **y** varias órdenes |

### Mediciones

| Conjunto | Resultado | Cómo leerlo |
|---|---|---|
| B (119, medido a ciegas en la v0.7.2 con un 83 %) | 119/119 | **Optimista**: B guió estas mejoras |
| **C (110 solicitudes nuevas, escritas después de las mejoras y medidas una vez)** | **90/110 (82 %)** | **Medición real** |
| Orquestador de un agente (v0.7, conjunto propio de 30) | 27/30 (antes 24/30) | Efecto secundario del vocabulario nuevo |

**Conclusión honesta:** las mejoras corrigieron los fallos conocidos, pero con datos nuevos el acierto se mantiene en torno al **82–83 %**. Las reglas por palabras clave parecen haber llegado a su techo. Los fallos restantes casi siempre tienen una salida segura:
- 13 de los 20 terminan en «responde un solo agente», como en la v0.7 (11 que necesitaban un plan y 2 que necesitaban una aclaración);
- los planes innecesarios siguen pidiendo permiso antes de hacer nada;
- de las 3 aclaraciones que faltaron, 2 acaban en un agente que pide confirmación antes de borrar y la tercera en un plan que también pide permiso.

### Fallos pendientes del conjunto C (20)

**Se resolvió con un solo agente en vez de con un plan (11):**
- «necesito que averigües el horario del museo y que lo apuntes en mis pendientes»
- «pon una rutina de estudio para las tardes y revisa que mi PC no se caliente cuando juego»
- «mira qué apps arrancan con Windows y crea una tarea para desactivar las innecesarias»
- «lee la documentación de la API en https://docs.ejemplo.com y escribe un ejemplo en Python»
- «busca recetas veganas fáciles y agenda una para cocinar el sábado»
- «organiza la carpeta Música por artista y luego reproduce algo de Queen»
- «busca mis notas de la clase de química y crea tareas para los temas del examen»
- «chécame si la compu se está calentando y pon una alarma para descansar en una hora»
- «investiga cómo se hace un README bueno y luego mejora el de mi proyecto» («mejora» no está en la lista de verbos)
- «revisa por qué tarda tanto en arrancar la PC y busca soluciones en internet»
- «dime cuánto pesa la carpeta de videos y busca cómo comprimir videos»

**Plan con un agente distinto (2):**
- «dime qué es un SSD, revisa cuánto espacio queda en el mío y anota…» → falta el agente de sistema.
- «busca qué significa el error 0x80070005…» → programadora en vez de investigadora.

**Plan innecesario, bastaba un agente (4, igual se pide permiso):**
- «**programa** un recordatorio… y pon música»
- «busca el PDF del manual del router y explícame cómo cambiar la contraseña del **wifi**»
- «revisa tu **memoria** de lo que te dije…» (la memoria de Canela, no la RAM)
- «haz una copia de mi carpeta de proyectos en el disco externo y **avísame** al terminar»

**Aclaración que faltó (3):**
- «bórrame todo lo que no sirva» (lo atendió un solo agente)
- «borra eso que te dije…» (lo atendió un solo agente)
- «vacía la carpeta…» (no dice cuál; creó un plan, que pide permiso)

Para mejorar sin trampa hará falta otro enfoque (por ejemplo, que el modelo proponga el plan con más frecuencia, siempre validado) y otro conjunto nuevo para medirlo.

---

## Parte 2 — v0.8: verificación de resultados

### Objetivo

Que Canela **no dé un trabajo por terminado porque la respuesta suene bien**, sino porque el programa comprobó que se hizo. Si sólo se hizo una parte, debe decir qué falló y cómo arreglarlo.

### Cómo funciona (`src-tauri/src/verification.rs`)

1. **Objetivos de la petición** (reglas explicables, sin llamar a ningún modelo; sólo con lo que escribiste, nunca con el contenido de adjuntos). Tipos de objetivo:
   - crear, eliminar o recordar tareas, con cantidad mínima («crea 3 tareas» exige 3);
   - crear carpetas, escribir archivos y organizar carpetas;
   - investigar (Wikipedia o páginas);
   - leer o revisar archivos y proyectos;
   - medir el equipo (también preguntas sin verbo, como «¿cuánta RAM uso?»);
   - controlar la música y abrir aplicaciones.
2. **Contraste con lo que REGISTRÓ el programa**:
   - Cada objetivo se cumple sólo con acciones reales y bien hechas.
   - Cada acción cuenta para un solo objetivo.
   - Si la petición nombra un archivo («app.js», «resumen.md»), sólo cuenta una acción sobre ese archivo.
3. **Comprobaciones extra**:
   - **Código** (sin ejecutar nada): JSON válido; paréntesis, llaves y corchetes equilibrados (ignorando textos y comentarios); comillas cerradas; en Python y YAML, sangría sin mezclar tabulaciones. Lenguajes: Rust, JS/TS, C/C++, C#, Java, Go, Kotlin, Swift, PHP, CSS, Python, shell, YAML, TOML y PowerShell. Si el archivo se guardó pero el código tiene un error evidente, **no cuenta como hecho**.
   - **Investigación**: la respuesta debe citar alguna fuente consultada, y **las cifras deben aparecer en las páginas leídas**. Si no, se avisa: «Estas cifras no aparecen en las fuentes leídas: 1870, 25».
4. **Resultado**:
   - **completo** (todo comprobado);
   - **parcial** (algo sí y algo no);
   - **no logrado** (nada de lo pedido se hizo);
   - **sin comprobar** (charla o explicación: no había acción que comprobar).
5. **Explicación y solución** de cada fallo, según su causa:

| Causa | Qué propone Canela |
|---|---|
| No lo autorizaste | Pedirlo de nuevo y pulsar Permitir |
| Carpeta no autorizada | Autorizarla en Ajustes › Permisos |
| No existe | Comprobar el nombre o la ruta |
| Error de sintaxis | Ofrece corregirlo |
| Archivo cambiado por otro programa | Revisarlo o escribirlo de nuevo |
| Error temporal | Reintentarlo |
| Acción de otro agente | Pedirlo de nuevo para que lo haga el agente adecuado |
| Límite de gasto | Subirlo en Ajustes |
| Sólo hubo texto | Pedirlo de forma concreta |

6. **Si la respuesta dice «listo / terminé / ya quedó» y la comprobación no lo confirma, se avisa.**
7. **Planes de varios pasos**:
   - cada paso se comprueba con su propia tarea;
   - un paso que no logró lo suyo queda **«no logrado»**;
   - el informe final reúne los resultados de todos los pasos.

### Lo que cambia en la práctica

- Antes, un paso en el que el modelo decía «Revisé el código: todo bien» **sin leer ningún archivo** contaba como «hecho». Ahora cuenta como **«no logrado: no se ejecutó ninguna acción, sólo hubo texto»**. Cinco pruebas de la v0.7.x se actualizaron por esto (estaban aceptando respuestas sin acción).
- La interfaz muestra bajo cada respuesta la tarjeta **«Resultado: completo ✓ / parcial ◐ / no logrado ✕»**, con cada objetivo, qué falló y «Cómo arreglarlo».

### Mediciones

| Medida | Resultado | Nota |
|---|---|---|
| Cobertura: solicitudes reales (conjuntos B y C, sin las de aclaración) con al menos un objetivo comprobable | **186/211 (88 %)** | Las 25 restantes son charla, preguntas («dime la hora»), peticiones sin herramienta («mándale un correo») o formas que el detector aún no reconoce («léela y agenda el pago», «hazme un reporte») |
| Veredicto correcto en 40 escenarios etiquetados (petición + acciones + respuesta) | **40/40**, **0 «completos» falsos** | Orientativo: los escenarios se escribieron después del verificador. La primera medición encontró un fallo: un archivo con otro nombre contaba como el pedido. Se corrigió de forma general y se corrigió también la etiqueta de ese escenario |
| Costo | ≈ 0,2 ms por respuesta; **sin llamadas extra al modelo**; memoria de la app ≈ 85 MB (igual que antes) | Versión optimizada / de depuración |

### Pruebas nuevas (231 de Rust, 62 de interfaz)

- **Respuestas sin acción**: «¡Listo! Ya creé la carpeta» sin hacerlo → no logrado + aviso.
- **Archivos**: escribir y comprobar el contenido; escribir fuera de las carpetas autorizadas → no logrado con explicación.
- **Programación**: `app.js` con una llave sin cerrar y `config.json` válido → parcial: «“{” de la línea 1 sin cerrar» y la propuesta de corregirlo. Esta prueba destapó y corrigió un fallo real: una sola acción correcta contaba para dos objetivos.
- **Investigación** (Wikipedia simulada): con la cifra y la fuente correctas → completo; con cifras inventadas y sin citar → parcial, con las cifras señaladas.
- **Varios pasos**: el paso 1 se cumple y el paso 2 crea 1 de 2 tareas → el paso 2 queda «no logrado» y el resultado conjunto es «parcial».
- **Errores temporales**: error del modelo después de crear una tarea → informe parcial (la tarea sí, la carpeta no).
- **Cancelación**: detener con un permiso pendiente → no se escribe nada, no hay informe de «terminado» y aprobar después no hace nada.
- Unitarias: objetivos, cantidades, resultados completo/parcial/no logrado, cifras frente a fuentes, comprobación de código (10 casos, incluidos textos y comentarios con llaves y los tiempos de vida de Rust), soluciones y unión de resultados de un plan.

### Limitaciones

- **La verificación comprueba lo que se puede comprobar sin ejecutar nada**:
  - que existan tareas, carpetas, archivos y recuerdos, y que su contenido sea el aprobado;
  - que se usaron las fuentes, y la sintaxis básica del código.
- **No comprueba** que el código funcione (no se ejecuta, por seguridad), ni que un texto sea correcto o completo, ni que una app realmente se abriera.
- Que un archivo ya esté bien de antemano no cuenta: si pides «corrige app.js» y Canela no lo escribe, el resultado es «no logrado» aunque no hubiera nada que corregir.
- El detector de objetivos usa reglas y puede no reconocer formas poco comunes; entonces el resultado es «sin comprobar» (no dice «completo»).
- Las cifras sólo se contrastan en investigaciones con fuentes leídas en esa misma respuesta.
- Todo está probado con modelos **simulados**: falta ver con Qwen3, Claude y ChatGPT reales cuántas veces los objetivos y los avisos aciertan en el uso diario.

---

# Cierre de la v0.8 (rama de desarrollo)

## 1. Estado real del proyecto (inventario)

| Función | Implementada | Pruebas automáticas | Probada en Windows real |
|---|---|---|---|
| Mascota pixel art, animaciones, panel | Sí | Sprites y comportamiento (interfaz) | Sí (v0.5) |
| Chat de texto con Claude, ChatGPT y Ollama | Sí | Servidores simulados (streaming, errores, herramientas) | Sí (v0.5) |
| Voz: whisper.cpp local, Piper, «Hola, Canela» | Sí | Con audio y modelos reales en Linux; en Windows, sólo compilación | Sí (v0.5) |
| Capturas autorizadas y visión | Sí | Redimensionado, recorte y permisos | Sí (v0.5) |
| Memoria, tareas, rutinas, documentos | Sí | Base de datos y herramientas | Sí (v0.5) |
| Diagnóstico de memoria, modos Equilibrado/Ahorro | Sí | Detector, descarga por inactividad | Sí (v0.5) |
| Enrutador de modelos y límite de gasto (v0.6) | Sí | Exactitud del clasificador, presupuesto, reservas | **No** |
| Detección de datos privados (v0.6.1) | Sí | Conjuntos etiquetados | **No** |
| Agentes, coordinación y planificador (v0.7.x) | Sí | Planes, permisos, cancelación, evaluación con 229 solicitudes | **No** |
| Verificación de resultados (v0.8) | Sí | 45 + 40 escenarios e integración con modelos simulados | **No** |
| Comprobaciones de código (cierre v0.8) | Sí | Con Node, Python y npm reales en Linux; en Windows, las mismas pruebas en GitHub Actions | **No** (sólo en el servidor de GitHub) |

Todo lo posterior a la v0.5 está probado con **modelos simulados**; con Qwen3, Claude y ChatGPT reales queda pendiente.

## 2. Deuda técnica encontrada

| Problema | Riesgo | Estado |
|---|---|---|
| Los instaladores experimentales decían «0.5.0» | Confundirlos con la versión estable | **Corregido**: ahora 0.8.0 (rama de desarrollo) |
| El verificador daba «completo» a acciones sin comprobar (abrir apps, música) y a investigaciones sin respaldo | Éxitos aparentes | **Corregido** (punto 3) |
| Rutinas y planes ejecutados por la IA no pasaban por la comprobación del resultado | Planes «hechos» a medias | **Corregido** (lo detectó una prueba nueva) |
| Al agotarse el tiempo, `npm test` podía dejar vivo el proceso de las pruebas | Procesos huérfanos | **Corregido**: se termina el árbol de procesos (grupo en Linux, `taskkill /T` en Windows) |
| `coordination.rs` (≈1 900 líneas) y `Chat.tsx` (≈1 400 líneas) muy grandes | Más difícil de mantener | Pendiente: separar en módulos |
| La capa entre interfaz y backend (`commands.rs`, ventanas, credenciales) no tiene pruebas automáticas | Errores de integración | Pendiente: pruebas de humo en GitHub Actions |
| Las acciones de GitHub usan Node 20 (aviso de obsolescencia) | Avisos; algún día fallará | Pendiente (no bloquea) |
| El planificador por reglas acierta ≈ 82 % con datos nuevos | Planes que faltan o sobran | Pendiente: nivel 2 (clasificación contextual) |

## 3. Verificador por tipo de tarea

**Resultados posibles** (los que pediste):
- **Completo y verificado** ✓
- **Parcial** ◐
- **No logrado** ✕
- **Hecho, pero no verificable con la evidencia disponible** ?
- **Sin comprobar**: charla, no había acción que comprobar.

Un éxito parcial nunca se presenta como total, y lo que no se puede comprobar no se presenta como verificado.

| Tipo | Qué se comprueba |
|---|---|
| **Archivos** | Que el archivo exista y que sea **el que se pidió** (si nombras «resumen.md», escribir otro no cuenta). El contenido debe ser el aprobado, sin errores evidentes de formato (JSON, sangría de YAML/Python). Organizar: que **no queden archivos sueltos** («7 de 10 movidos» = parcial) |
| **Programación** | Sintaxis básica interna. Además, la herramienta nueva **`comprobar_codigo`** ejecuta comprobaciones reales de un **catálogo cerrado**: `node --check` y Python (sólo analizan, no ejecutan el programa), `npm test`, `cargo test`, `pytest`, `npm run build`, `cargo check` y TypeScript. «Corrige y comprueba que funciona» sin ejecutar la comprobación = **parcial** |
| **Investigación** | **Procedencia**: debe citar fuentes que se consultaron de verdad (citar otra = fallo). **Respaldo** de cada afirmación frente a las páginas leídas. Coincidir con la fuente **no prueba** que algo sea verdad, y una afirmación o cifra sin respaldo queda **«no verificable»**, no «falsa» |
| **Tareas y recordatorios** | Que la tarea exista con el título, **la fecha y el recordatorio** pedidos. Editar o completar: que los cambios estén guardados. Cantidades («crea 3» y se crearon 2 = parcial) |
| **Automatizaciones** | Cuántos pasos se completaron de verdad («2 de 3» = parcial; «0 de 3» = no logrado) |
| **Consultas** (leer, buscar, medir el equipo, listar tareas) | El registro de la consulta es la evidencia. **Responder datos sin haber consultado = no logrado** («Tienes 3 tareas» sin mirarlas) |
| **Abrir apps, música, carpetas** | Se hacen, pero su efecto no se puede comprobar: **«sin verificar»**, nunca «completo» |

Además, cada resultado se apunta en **Actividad** («verificación»), para revisar errores y medir más adelante.

### Seguridad de `comprobar_codigo`

- **El modelo no escribe comandos**: sólo elige el tipo (sintaxis, pruebas, compilar) y una ruta dentro de las carpetas autorizadas. El programa decide el comando según los archivos del proyecto (`package.json`, `Cargo.toml`, `pytest.ini`…). Un campo «comando» inventado por el modelo se ignora; una prueba lo demuestra.
- **Sin intérprete de comandos**: el programa se lanza directamente, con argumentos fijos.
- **Siempre pide permiso** mostrando el comando exacto y la carpeta. Si ejecutará código del proyecto, lo advierte. La revisión interna de sintaxis no lanza ningún proceso.
- Se **quitan del entorno** las variables con aspecto de secreto (KEY, TOKEN, SECRET, PASSWORD…); una prueba lo comprueba.
- **Límite de tiempo** (30 s la sintaxis, 300 s las pruebas). Al agotarse o al pulsar Detener, se termina **todo el árbol de procesos**.
- La salida se recorta, se ocultan claves y se trata como **datos, no instrucciones**. Lo que se haga después de leerla pide confirmación.
- Sólo la usan la programadora y Canela general.

**Limitación honesta:** si apruebas «pruebas» o «compilar», se ejecuta el código de tu proyecto con tus permisos de usuario. No hay un entorno aislado (sandbox) ni se bloquea internet en esos procesos. Por eso siempre se pregunta antes.

## 4. Mediciones

| Medida | Resultado |
|---|---|
| **Escenarios nuevos del cierre (45), escritos ANTES de cambiar el verificador** | Antes: **27/45 (60 %), 7 «completos» falsos**. Después: **45/45, 0 «completos» falsos** (no es a ciegas: el mismo equipo escribió los escenarios y el verificador) |
| Escenarios anteriores (40) | 40/40. Se reetiquetaron 5 que tenían categorías anteriores a las pedidas («pon música» era «completo»; ahora es «sin verificar»), con un comentario en el código |
| Pruebas | **244 de Rust** (antes 231) y **64 de interfaz** (antes 62); suite repetida 3 veces sin fallos |
| Comprobaciones reales en Linux | `node --check` y Python no ejecutan el programa (un archivo marcador lo demuestra). `npm test` no ve las claves del entorno. El límite de tiempo termina el proceso hijo de npm |
| Costo | Verificación ≈ 0,2 ms y **sin llamadas extra al modelo**. Las comprobaciones de código sólo se ejecutan si se piden y se aprueban |

## 5. Qué sigue (propuesta, sin fechas)

1. **v0.8.1 (calidad)**: separar `coordination.rs` y `Chat.tsx`, pruebas de humo de la app en GitHub Actions y actualizar las acciones de GitHub.
2. **v0.9 (memoria semántica)**: búsqueda por significado con un modelo local pequeño de embeddings (descarga con permiso), duplicados, contradicciones, recuerdos antiguos o inciertos y memoria de proyectos.
3. **v0.10 (investigación)**: búsqueda web real con varias fuentes, comparación y contradicciones entre fuentes.
4. **v0.11 (evaluación)**: batería reproducible con modelos reales (calidad, latencia, memoria, costo) en tu PC.
5. **Planificador nivel 2**: clasificación contextual (intención, entidades, dependencias) antes de recurrir al modelo.
