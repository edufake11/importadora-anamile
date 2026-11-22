# CANELA AI — v0.7: agentes especializados (rama de desarrollo)

Estado: **arquitectura + primera implementación funcional en la rama de desarrollo**. No es una versión estable: falta probarla en Windows 11 real. La versión validada sigue siendo la **v0.5**.

## 1. Idea

Un **agente** no es otro chat ni otro modelo cargado en memoria. Es un **perfil**: instrucciones + lista de herramientas permitidas. Para cada mensaje, un **orquestador** elige el perfil y la respuesta sigue en la **misma conversación** y con el **mismo modelo** que eligió el enrutador (v0.6). Así no se cargan varios modelos grandes a la vez.

```
Mensaje ─► Enrutador (modelo, privacidad, presupuesto)
              │
              ▼
         Orquestador ─► agente: programadora · investigadora · productividad · archivos · sistema · Canela general
              │
              ▼
         Bucle de agente ─► herramientas permitidas al agente ─► confirmación (si es delicada) ─► ejecución
              │                                                                                    │
              │                                                       Verificación objetiva ◄──────┘
              ▼
         Informe «Qué hice» + avisos (afirmaciones sin acción, fuentes no consultadas, comprobaciones fallidas)
```

## 2. Agentes y permisos

| Agente | Herramientas (las demás no las ve ni puede usarlas) |
|---|---|
| Programadora | leer/listar/buscar archivos, leer documentos, escribir archivo (con diferencias y aprobación), crear carpeta, analizar proyecto, abrir con VS Code, índice de documentos, recuerdos, planes |
| Investigadora | **buscar_wikipedia**, **leer_pagina** (nuevas), abrir web, buscar/leer documentos propios, recuerdos |
| Productividad | tareas (crear, listar, actualizar, eliminar), recuerdos, conversaciones, planes y rutinas, abrir aplicaciones, multimedia |
| Archivos | listar, buscar, recientes, leer, ver imagen, crear carpeta, organizar (sólo mueve), escribir, abrir, índice de documentos. **No tiene herramienta de borrar.** |
| Sistema | info_sistema, **diagnostico_memoria** (nueva, sólo lectura), multimedia, abrir aplicaciones, ver pantalla (con permiso) |
| Canela general | todas (como en v0.6), para charla o cuando el orquestador no está seguro |

**Garantías comprobadas en el backend** (no dependen de que el modelo obedezca):
- El modelo sólo recibe las definiciones de las herramientas de su agente.
- Si aun así pide otra, se rechaza («no permitida») y queda en el informe.
- Un plan (`ejecutar_plan`) tampoco puede incluir herramientas de otro agente: se rechaza el plan entero antes de ejecutar ningún paso.
- Todas las confirmaciones existentes siguen igual (borrar, escribir, mover, capturas, enviar datos fuera).

## 3. Contexto compartido de forma controlada

- Todos los agentes ven la conversación (lo que escribiste y las respuestas finales).
- Los **datos internos de las herramientas** (contenido de archivos leídos, resultados intermedios) **sólo los vuelve a ver el mismo agente** que los obtuvo. Si cambia el agente, sólo pasa la respuesta final.
- Mensajes cortos de seguimiento («sí, hazlo», «¿y el otro?») mantienen el agente anterior.
- Con un modelo externo, todo lo leído por herramientas sigue pasando por el filtro de privacidad de la v0.6.1.

## 4. Nada de «fingir que trabaja»

- **Registro real**: cada acción que se ejecuta (o se rechaza, o deniegas) queda registrada por el backend, no por el modelo.
- **Verificación objetiva** después de actuar:
  - crear tarea → la tarea existe en la base de datos con ese título;
  - eliminar tarea → ya no existe;
  - escribir archivo → el archivo existe y su contenido es exactamente el aprobado;
  - crear carpeta → la carpeta existe;
  - recordar → el recuerdo está guardado;
  - guardar rutina → la rutina existe.
  Las demás acciones (abrir una app, leer) se marcan como «no comprobable» en vez de inventar una comprobación.
- **Informe «Qué hice»** bajo la respuesta: acciones hechas, cuántas comprobadas, cuántas fallaron la comprobación y cuántas no se hicieron.
- **Avisos automáticos**:
  - si la respuesta dice «ya lo guardé / creé / borré…» pero no se ejecutó ninguna acción;
  - si la investigadora cita una URL que no consultó en esa respuesta;
  - si una comprobación falla.

## 5. Herramientas nuevas (investigación y sistema)

- `buscar_wikipedia` (API pública de Wikipedia) y `leer_pagina`: sólo `http/https` públicos; se rechazan `localhost`, redes privadas (10.x, 192.168.x, 172.16–31.x, 169.254.x, `::1`, `fc00::/7`, `.local`, `.internal`), direcciones con usuario/contraseña; la comprobación se repite en cada redirección (máx. 5) y **al resolver el nombre**: un dominio público que apunte a tu red local (técnica de «DNS rebinding») también se bloquea. Máx. 2 MB y 15 s; se quita el HTML (scripts, estilos) y se recorta a 20 000 caracteres. El contenido se trata como **externo** (no puede dar órdenes) y la consulta se revisa con el filtro de privacidad. Piden confirmación salvo que hayas activado abrir webs sin preguntar.
- `diagnostico_memoria`: lee la última muestra del diagnóstico de la v0.5 (RAM, commit, procesos por nombre, sin rutas) y los últimos eventos. Sólo lectura.

## 6. Evaluación (automática, Linux, modelos simulados)

| Prueba | Resultado |
|---|---|
| Orquestador, conjunto usado al desarrollarlo (30 mensajes) | 29/30 (97 %, optimista) |
| **Orquestador, conjunto nuevo escrito después y medido una vez sin reajustar (30)** | **25/30 (83 %)**. Los 5 fallos fueron a «Canela general» (que tiene todas las herramientas), ninguno a un agente equivocado |
| Cada agente sólo ve sus herramientas (petición real al modelo simulado) | ✔ |
| Herramienta ajena pedida por el modelo → rechazada, sin efecto | ✔ |
| Plan con un paso ajeno → rechazado entero, ningún paso ejecutado | ✔ |
| Crear tarea → comprobada en la base de datos y en el informe | ✔ |
| «Ya creé la tarea» sin acción → aviso | ✔ |
| Fuente citada sin consultar → aviso | ✔ |
| Datos internos de otro agente no se comparten; los del mismo, sí | ✔ |
| Seguridad de `leer_pagina` (15 direcciones peligrosas rechazadas; nombre que resuelve a 127.0.0.1 bloqueado) | ✔ |
| **En la app (Linux, Ollama simulado)**: «crea una tarea…» → «asistente de productividad», 14 herramientas enviadas, «✓ Crear tarea · comprobado», «Qué hice: 1 acción hecha · 1 comprobada» | ✔ |
| **En la app**: respuesta que dice «ya guardé el informe» sin acción → «agente de archivos» + aviso visible | ✔ |

## 7. Pendiente

- **Probar en Windows 11 real** con Qwen3 y con Claude/ChatGPT: que el modelo real use bien las herramientas de cada agente y que los avisos no molesten.
- Que la investigadora funcione con internet real (en las pruebas, Wikipedia está simulada).
- Mejorar el orquestador con los fallos del conjunto nuevo (en un conjunto distinto para volver a medir sin trampa).
- v0.8 (siguiente del plan): verificación más amplia (¿compila?, ¿pasan las pruebas?) y reintentos acotados.

---

# v0.7.1 — Coordinación de varios agentes (rama de desarrollo)

Para encargos como **«Canela, revisa mi proyecto web, identifica errores, organiza las tareas pendientes y prepara un informe»**, ya no hace falta elegir agentes: Canela arma un plan y reparte el trabajo. Código: `src-tauri/src/coordination.rs`.

## Cómo funciona

1. **Plan** (reglas explicables, sin llamar a un modelo para decidir): divide el encargo en partes sólo donde empieza una orden nueva («revisa…», «organiza…», «que busques…»), elige el agente de cada parte y junta las partes seguidas del mismo agente. Si pide «informe», «resumen» o «conclusiones», añade un paso final de redacción **sin herramientas**. Sólo se crea un plan si hacen falta **al menos dos agentes distintos** (máx. 6 pasos); si no, responde un agente como en la v0.7.
   - Ejemplo → 1. programadora: «Revisa mi proyecto web; identifica errores» · 2. productividad: «Organiza las tareas pendientes» · 3. Canela (informe): «Prepara un informe».
2. **Permiso**: una tarjeta muestra el plan completo y pide aprobarlo una vez. Dentro de cada paso, lo delicado (escribir, borrar, mover, capturas, pasar un archivo a la IA) se vuelve a preguntar como siempre.
3. **Delegación**: cada paso usa **el mismo modelo** (no se cargan otros) y **sólo las herramientas de su agente**.
4. **Contexto mínimo**: cada paso recibe el objetivo, su parte, el texto reciente de la conversación (sin datos de herramientas) y los **resúmenes** de los pasos anteriores (máx. 1 500 caracteres cada uno). El contenido de los archivos que leyó un paso no llega a los demás.
5. **Comprobación**: las acciones se verifican como en la v0.7; un paso con una acción denegada, no permitida o cuya comprobación falló queda **«incompleto»**.
6. **Recuperación ante errores**: un error temporal (servidor ocupado, red, conexión cortada) se **reintenta una vez**, salvo que ese intento ya hubiera hecho cambios: entonces **no se repite** (para no duplicar tareas, archivos…) y se informa qué se llegó a hacer. Un paso con error no detiene a los demás: los siguientes saben que falló y el informe lo dice.
7. **Presupuesto**: el gasto real se suma paso a paso; al llegar al límite, los pasos restantes quedan pendientes sin más llamadas de pago. Antes de empezar se reserva el costo estimado de todos los pasos.
8. **Detener**: si pulsas Detener en cualquier momento, no se hace nada más y el informe muestra qué se hizo, qué se canceló y qué quedó pendiente.
9. **Informe final** bajo la respuesta: estado de cada paso y tres listas — **Hecho**, **Pendiente** y **Errores** — construidas sólo con lo que registró el programa (no con lo que dice el modelo).

## Evaluación (automática, Linux, modelos simulados)

| Prueba | Resultado |
|---|---|
| Planificador, conjunto usado para ajustarlo (8 encargos) | 8/8 (optimista; antes del ajuste 5/8) |
| **Planificador, conjunto nuevo escrito después y medido una vez (15 encargos, 4 de un solo agente)** | **12/15 (80 %)**. En los 3 fallos no se creó plan y respondió un solo agente, como en la v0.7; ninguno produjo un plan equivocado. Causas: el planificador no reconoce verbos con pronombre pegado («guárdala», «explícame»), así que no corta ahí, y tampoco corta en «y que agendes» (el «que» entre «y» y el verbo) |
| Ejemplo completo: 1 permiso de plan + permiso para pasar el archivo a la IA, 3 agentes en orden, mismo modelo en las 5 llamadas, herramientas correctas por paso, informe sin herramientas, el contenido del archivo no llega a otros pasos, tarea comprobada, una sola respuesta en el chat | ✔ |
| Plan denegado → ninguna llamada al modelo, todo pendiente | ✔ |
| Acción denegada dentro de un paso → no se hace, paso «incompleto», el informe lo sabe | ✔ |
| Detener a mitad (mientras pide permiso para crear una carpeta) → carpeta no creada, informe «hecho / cancelado / pendiente», sin más llamadas | ✔ |
| Error 500 → reintento con éxito; error 500 dos veces → paso con error y el plan continúa | ✔ |
| Error después de crear una tarea → no se reintenta (la tarea no se duplica) | ✔ |
| Límite de gasto superado en el paso 1 → el paso 2 queda pendiente sin llamada de pago | ✔ |
| Suite completa repetida 8 veces seguidas sin fallos (para descartar pruebas inestables) | ✔ |
| **En la app (Linux, Ollama simulado)**: el ejemplo del usuario → tarjeta del plan, permiso, 3 pasos con su agente (14, 14 y 0 herramientas), «✓ comprobado», informe «Hecho» y «Plan: 3 de 3 pasos hechos» | ✔ |
| **En la app**: Detener mientras pide aprobar el plan → 0 llamadas al modelo e informe con los 2 pasos pendientes | ✔ |

Consumo medido en la app de prueba (versión de depuración): proceso principal ≈ 86 MB, igual que en la v0.7; CPU en reposo ≈ 4 %. Los pasos se ejecutan uno tras otro con el mismo modelo, así que no aumenta la memoria del modelo.

## Pendiente

- Probar en Windows 11 real con Qwen3 y con Claude/ChatGPT: que el modelo real haga bien su parte y resuma lo necesario.
- Planes propuestos por el propio modelo para encargos que las reglas no detectan (hoy esos los atiende un solo agente).
- Corregir los 3 fallos del conjunto nuevo (verbos con pronombre pegado, «y que …») y medir con otro conjunto distinto.

---

# v0.7.2 — Planificador más natural, planes del modelo validados y seguridad (rama de desarrollo)

## 1. Español natural

El planificador (`src-tauri/src/coordination.rs`) ahora reconoce:
- **Pronombres pegados**: «guárdala», «explícame», «revísalo», «házmelo», «dímelo», «agrégalas», «prográmeme».
- **Subjuntivo y trato de usted**: «que busques», «que agendes», «revise», «organice», «busque».
- **Presente usado como orden**: «y luego lo guardas».
- **«y que …» y pronombres entre el conector y el verbo**: «y que me expliques», «y luego lo guardas».
- **Listas** numeradas («1.», «2)», «a)», «-») y encargos escritos en varias líneas.
- **Verbos irregulares**: «haga», «ponga», «corrijas», «probar», «mostrar», «traduzca»…
- No confunde **pasados** («revisé», «abrí») ni **sustantivos** («listas», «programas», «escuela», «pantalla») con órdenes.

Además se corrigió un error que venía de la v0.7: las palabras clave se buscaban como trozos de palabra («ram» dentro de «prog**ram**a», «api» dentro de «c**apí**tulo»). Ahora se buscan al inicio de una palabra y las muy cortas («ram», «api», «cpu») sólo como palabra completa.

## 2. Seguridad antes de planificar

Antes de crear un plan, Canela comprueba que el encargo se entiende. Si no, **pregunta y no hace nada** (ni siquiera llama al modelo):
- **Instrucciones contradictorias**: «crea una tarea… y no crees ninguna tarea» → «Me pides «crea» y también que no lo haga. ¿Qué prefieres?». «No olvides revisar» no cuenta como contradicción.
- **Acciones destructivas sin objeto concreto**: «bórralo todo», «elimina eso», «limpia todo lo demás», «vacíalo» → pregunta qué exactamente. Con un objeto concreto («borra la tarea 3») sí planifica, y borrar sigue pidiendo permiso.
- **Más de 6 partes**: propone dividir el encargo.
- **Archivos adjuntos**: si el mensaje trae archivos, **no se planifica**. El contenido de un archivo nunca se convierte en pasos ni en órdenes. Este riesgo existía en la v0.7.1: el texto adjunto se analizaba junto con el mensaje. Aunque cualquier plan pedía permiso, ya está cerrado.
- **Peticiones ambiguas** («haz lo de siempre y lo otro también», «revisa eso y arréglalo»): no se convierten en un plan; responde un solo agente, que puede preguntar.

## 3. Planes propuestos por el modelo (estudio + implementación con validación)

**Cuándo:** sólo si las reglas no encuentran un plan, pero el mensaje menciona **varios especialistas** y tiene **al menos dos órdenes** (p. ej. «revisa el código de la API cuando puedas porque mañana quiero crear una tarea con lo que encuentres»). En el resto de los mensajes no hay consulta extra.

**Cómo, sin riesgo:**
1. Se hace **una consulta al mismo modelo, sin herramientas** (no puede ejecutar nada). Sólo recibe el texto del usuario, marcado como datos que hay que planificar.
2. La respuesta (JSON) **no se muestra** y el programa la **valida**:
   - debe ser JSON con 2 a 6 pasos y sólo agentes que existen;
   - cada tarea, de 3 a 300 caracteres y sin caracteres de control;
   - **ninguna acción destructiva** (borrar, eliminar, vaciar, limpiar, mover, renombrar) que no esté en tu petición;
   - **ninguna dirección web** ni **dato de acceso** (contraseña, token, clave) que no estén en tu petición;
   - cada paso debe corresponder a algo de tu petición (la misma palabra, el mismo verbo —«crear» ≈ «crea»— o la misma raíz): el modelo no puede inventar tareas, pero puede reformularlas;
   - si el modelo escribe rutas de Windows sin escapar («C:\Users\…»), el JSON se repara antes de validarlo;
   - el informe va al final y sólo si lo pediste (si no, se quita y se avisa);
   - si un paso está asignado a un agente que no corresponde, Canela lo reasigna y lo dice.
3. Si el plan no pasa la validación, **se descarta**, responde un solo agente y se avisa del motivo («No usé el plan que propuso el modelo: el paso 2 añade «borra», que no pediste»).
4. Si pasa, se muestra la tarjeta **«Plan de trabajo con N pasos (propuesto por el modelo)»** con las correcciones de Canela. **Nada se ejecuta hasta que lo apruebas**, y dentro de cada paso siguen todas las confirmaciones.
5. El costo de la consulta de planificación se suma al de la respuesta y al gasto del mes.

**Costo:** una consulta corta más, sólo en los mensajes que lo necesitan. En la evaluación (punto 5), 3 de 119 mensajes la habrían usado con acierto y 1 innecesariamente.

**Límite conocido:** aún no se ha probado con modelos reales (Qwen3, Claude, ChatGPT). En las pruebas el modelo está simulado, así que no sabemos cuántas veces un modelo real propondrá un plan válido.

## 4. Pruebas nuevas

| Tipo | Pruebas |
|---|---|
| Español natural | formas verbales (17 aciertos y 10 palabras que NO son órdenes), cortes con «y que», «luego lo guardas», listas numeradas y en líneas, frases que fallaban en la v0.7.1 |
| Planes mal formulados (del modelo) | texto sin JSON, JSON roto, sin «pasos», pasos que no son una lista, 0 o 1 paso, agente inexistente, sin agente, sin tarea, tarea vacía, demasiado larga, con caracteres de control, más de 6 pasos |
| Planes peligrosos (del modelo) | borrar o mover algo no pedido, una URL no pedida, una contraseña, una tarea inventada («poner música»), un agente equivocado (se corrige), un informe no pedido (se quita) |
| Instrucciones contradictorias | 3 casos que piden aclaración; «no olvides revisar» y «no borres nada» no son contradicción |
| Solicitudes ambiguas | 3 acciones destructivas sin objeto piden aclaración, 4 peticiones vagas no se convierten en plan, un encargo de 8 partes pide dividirlo, adjuntos con órdenes dentro no generan plan |
| Cancelaciones | Detener mientras se aprueba un plan del modelo y aprobarlo después: no se ejecuta nada. Detener con un permiso pendiente dentro de un paso y aprobarlo después: la carpeta no se crea |
| Integración (modelo simulado) | plan del modelo aceptado (consulta sin herramientas, JSON oculto, permiso antes de ejecutar, costo sumado); plan peligroso descartado (nada se borra, aviso, sigue un solo agente); respuesta que no es un plan (sigue un solo agente); aclaración sin llamar al modelo |

Total: **211 pruebas de Rust** (antes 189) y 59 de interfaz. La suite completa se repitió varias veces seguidas sin fallos.

## 5. Evaluación con 119 solicitudes nuevas (`src-tauri/src/coordination_eval.rs`)

Las solicitudes se escribieron después de terminar las reglas y se etiquetaron a mano con el resultado ideal: 74 necesitan varios agentes, 36 basta uno y 9 requieren aclaración. Incluyen usted, pronombres pegados, «y que», listas, saltos de línea, expresiones coloquiales («chécame», «ahorita», «la compu») y trampas («administrador de tareas», «no borres nada»).

| | Primera medición (oficial, a ciegas) | Tras corregir el error de palabras clave (optimista) |
|---|---|---|
| **Aciertos de las reglas** | **99/119 (83 %)** | 102/119 (86 %) |
| Planes no resueltos por las reglas que pasarían al modelo (con validación) | 3 | 3 |
| Planes con agentes distintos de los esperados | 6 | 3 |
| Planes innecesarios (bastaba un agente; igual se pide permiso) | 2 | 2 |
| Consultas extra innecesarias al modelo | 1 | 1 |
| Aclaraciones que faltaron | 1 | 1 |

La segunda columna es optimista: la corrección se hizo después de ver los resultados, aunque es una corrección general y no un ajuste a estas frases. Después de eso **no se tocaron las reglas** para este conjunto.

Efecto en el orquestador de la v0.7 (un solo agente): su conjunto nuevo pasa de 25/30 a 24/30. «¿Qué **programa** está usando más memoria?» antes iba al agente de sistema sólo porque «ram» aparecía dentro de «programa», es decir, acertaba por casualidad. Ahora va a Canela general, que tiene todas las herramientas.

### Fallos que quedan pendientes (17 después de la corrección)

**Palabras o temas que el planificador aún no reconoce** (7 solicitudes, que se resolvieron con un solo agente en vez de con un plan):
- Órdenes que no reconoce como tales: «apaga», «toma nota».
- «busca en internet» sin la palabra «investiga» (2 casos).
- «revisa mi **Dockerfile**», que no reconoce como código.
- «se **calienta**», que no reconoce como tema del sistema.
- «mis **apuntes**», que no reconoce como archivos.
- «apunta que debo comprar más **RAM**»: la palabra «RAM» hace que la anotación vaya al agente de sistema.

**Condiciones («si está alta…», «y si fallan…») y «¿me ayudas a … y después a …?»**
- No corta ahí. Estos 3 casos y «lee el archivo README del proyecto y explícame cómo instalarlo» acaban consultando al modelo: los 3 primeros con razón y el último innecesariamente.

**Palabras que pertenecen a dos agentes**
- «el documento del proyecto» → programadora en vez de archivos.
- «el archivo de paginación» → archivos en vez de investigadora.
- «búscalos en internet» → se queda en la programadora.

**Planes innecesarios**
- «abre el administrador de **tareas** y revisa la memoria» → crea un plan con productividad.
- «lee mi archivo de notas y dime qué **tareas** tengo apuntadas ahí» → también.
- En ambos casos se pide permiso para el plan antes de hacer nada.

**Aclaración que faltó**
- «mueve **eso** a la otra carpeta y crea un recordatorio» → planifica en vez de preguntar qué mover. Lo mitiga que el plan y la acción de mover piden permiso igualmente.

Para corregirlos sin trampa hará falta otro conjunto nuevo.

## 6. Recursos

- Planificador: ≈ 1 ms por mensaje (las 119 solicitudes en 0,13 s, versión optimizada). No usa el modelo.
- La consulta de planificación al modelo sólo ocurre en los mensajes que la necesitan (3–4 de cada 119 en la evaluación). Usa el mismo modelo ya cargado, sin herramientas y con una respuesta corta.
- En la app (Linux, versión de depuración): proceso principal ≈ 86 MB, CPU en reposo ≈ 2–4 %, igual que en la v0.7.1.
- **Probado en la app (Linux, Ollama simulado):** un mensaje contradictorio recibió la aclaración sin llamar al modelo (0 llamadas). Un mensaje que las reglas no resolvían mostró la tarjeta «propuesto por el modelo» con la revisión de Canela; tras aprobarla, 2 pasos hechos y la tarea comprobada. En total hubo 4 llamadas: 1 para planificar y 3 para los pasos.

## 7. Lo que encontró la compilación en Windows (GitHub Actions)

La primera ejecución en Windows de la v0.7.2 (commits 12adec6 y 3400294) falló en **1 de 208 pruebas**. La prueba metía una ruta de Windows (`C:\Users\…`) en el JSON del plan simulado sin escapar las barras. El plan se rechazaba igual y nada se borraba, pero por otro motivo («JSON no válido»). Se corrigió la prueba y, como un modelo real podría escribir rutas así, también el validador:
- repara las barras sin escapar antes de rendirse;
- acepta tareas reformuladas por el modelo («Crear una tarea con lo encontrado» frente a «crea una tarea con lo que encuentres»), y sigue rechazando las inventadas.
