# CANELA AI — Plan v0.6 y posteriores

Estado: **propuesta + primer módulo en desarrollo**. Nada de esto es una versión estable hasta probarlo en Windows 11 real.

## 1. Qué existe ya (v0.5) y se reutiliza

| Pieza | Dónde | Sirve para |
|---|---|---|
| 3 proveedores con streaming y herramientas (Ollama, OpenAI, Anthropic) | `ai/{ollama,openai,anthropic}.rs`, `ai::complete` | Base del enrutador multimodelo |
| Bucle de agente con 31 herramientas, permisos, confirmaciones y defensa contra instrucciones escondidas | `ai/agent.rs`, `tools/` | Ejecutar tareas reales |
| Planes de hasta 10 pasos con re-confirmación y rutinas | `tools::ejecutar_plan`, `agent::run_plan` | Planificación por etapas |
| Memoria SQLite (recuerdos, temporales, tareas, historial, índice FTS5 de documentos) | `db.rs` | Base de la memoria inteligente |
| Costos por respuesta | `ai/pricing.rs` | Control de gasto |
| Gestión de recursos (descarga por inactividad, modos, diagnóstico) | `resources.rs`, `memdiag.rs` | Rendimiento |
| Trabajos en segundo plano cancelables | `jobs.rs` | Ejecución asíncrona y supervisión |
| Voz y visión locales | `voice/`, `vision*.rs` | Multimodalidad |

## 2. Qué falta respecto al objetivo

1. **Elegir el modelo adecuado para cada mensaje** (hoy el proveedor es fijo en Ajustes).
2. **Límite de gasto** real (hoy sólo se muestra el costo).
3. **Agentes especializados** dentro de una sola conversación.
4. **Verificación** de resultados (hoy el plan se detiene al primer error, pero no comprueba que el resultado sea correcto).
5. **Memoria semántica** (por significado, no sólo por palabras), detección de duplicados y contradicciones.
6. **Investigación en internet con fuentes** (hoy sólo abre el navegador).
7. **Caché** de resultados y gestión más fina del contexto.
8. **Evaluación objetiva** frente al uso directo de los modelos.

## 3. Arquitectura propuesta (modular, sin rehacer nada)

```
Mensaje ─► [Enrutador] ─► elige modelo (local / externo con permiso) y límite de gasto
              │
              ▼
         [Orquestador] ─► elige el «agente» (perfil: instrucciones + subconjunto de herramientas)
              │               programador · investigador · productividad · archivos · sistemas
              ▼
         [Bucle de agente existente] ─► herramientas con permisos ─► [Verificador]
              │                                                        │
              └──────────── [Memoria] ◄────────────────────────────────┘
                                │
                        [Evaluación y registro]
```

- Un **agente** no es otro chat ni otro modelo cargado: es un perfil (instrucciones + herramientas permitidas + criterios de verificación) que el orquestador activa dentro de la misma conversación. Así no se cargan varios modelos grandes a la vez.
- Cada módulo se puede probar por separado con servidores simulados, como ya se hace con Ollama, OpenAI y Claude.

## 4. Versiones pequeñas (por utilidad real)

| Versión | Módulo | Qué aporta | Cómo se verifica |
|---|---|---|---|
| **v0.6** | **Enrutador de modelos + límite de gasto** | Lo sencillo se queda en Qwen3 (gratis, privado); lo difícil propone Claude/ChatGPT **con tu permiso y su costo estimado**; datos privados nunca salen; límite mensual real | Pruebas unitarias de la política, conjunto etiquetado de mensajes (exactitud del clasificador), registro de gasto, pruebas de bloqueo |
| v0.7 | Orquestador de agentes | Perfiles programador / productividad / archivos / sistemas (el investigador llega en v0.9) con herramientas acotadas | Pruebas: cada perfil sólo ve sus herramientas; el orquestador elige bien en un conjunto etiquetado |
| v0.8 | Verificación y recuperación | Tras cada paso, comprobaciones objetivas (¿se creó el archivo?, ¿compila?, ¿pasan las pruebas?); reintento acotado de errores recuperables; resumen final | Planes simulados con fallos inyectados |
| v0.9 | Memoria 2 | Búsqueda semántica con un modelo local de *embeddings* (descarga con permiso), duplicados y contradicciones, memoria de proyectos | Conjunto de consultas con respuestas esperadas |
| v0.10 | Investigador | Búsqueda web con fuentes citadas (proveedor a elegir), contenido tratado como externo | Respuestas con enlaces comprobables; nada se ejecuta por lo que diga una web |
| v0.11 | Evaluación comparativa | Batería de tareas: Canela vs. el mismo modelo usado directamente (calidad, exactitud, tiempo, memoria, costo, éxito de automatizaciones) | Informe reproducible con los resultados reales, sin cifras inventadas |
| Después | Caché de respuestas y herramientas, contexto por resúmenes, más integraciones | Velocidad y costo | Medición antes/después |

**Sobre «ser mejor que ChatGPT o Claude»**: Canela usa esos mismos modelos (o uno local más pequeño). Puede ser **más práctica** (herramientas reales, memoria, voz, privacidad, costo controlado), pero no más inteligente que el modelo que usa. La v0.11 medirá en qué tareas mejora la experiencia y en cuáles no.

## 5. v0.6 — Enrutador (primer módulo, implementado en la rama de desarrollo)

**Cuándo se activa:** cuando tu cerebro principal es Ollama y está activada la opción «Proponer Claude/ChatGPT para tareas difíciles» (Ajustes → Asistente). Si eliges Claude o ChatGPT como principal, se usa siempre ese, con el límite de gasto.

**Decisión por mensaje** (reglas explicables, sin llamar a ningún modelo para decidir):

1. **Privacidad primero:** si el mensaje parece contener contraseñas, claves, tarjetas o documentos de identidad, **sólo** se usa el modelo local. Si Ollama no está disponible, Canela no lo envía a ninguna parte y lo explica.
2. **Capacidad:** si adjuntas una imagen y el modelo local no ve imágenes, se propone un modelo externo con visión.
3. **Disponibilidad:** si Ollama no responde, se propone el modelo externo (con permiso).
4. **Complejidad:** mensajes sencillos o normales → local. Mensajes complejos (código largo, análisis, comparación, planes, depuración, varias preguntas) → local por defecto, con la **propuesta** de usar Claude o ChatGPT y su costo estimado.
5. **Gasto:** nunca se propone ni se usa un modelo externo si supera tu límite mensual (5 US$ por defecto, editable).
6. **Un solo modelo por mensaje:** no se consultan varios a la vez.

**Permiso:** la propuesta aparece en el chat: «Usar Claude (≈ 0,03 US$)» o «Seguir en local». Puedes permitirlo para el resto de la conversación.

**Registro de gasto:** cada respuesta de pago se guarda en la base de datos local (fecha, proveedor, modelo, tokens, costo aproximado) para el límite mensual.

### Estado de la implementación (rama de desarrollo, no estable)

- Código: `src-tauri/src/router.rs` (política y clasificador), `db.rs` (tabla `api_spend`, esquema v4), `commands::chat_send` (modelo por mensaje con control final de presupuesto y privacidad en el backend), `src/panel/Chat.tsx` (tarjeta de permiso), `src/panel/RouterSettings.tsx`.
- **Exactitud del clasificador** (pruebas automáticas, mensajes en español etiquetados a mano):
  - conjunto usado para ajustar las reglas: 35/35 (optimista por definición);
  - **conjunto nuevo no usado para ajustar: 19/20 (95 %)**; el fallo («analiza estos datos de ventas…») se clasificó como normal en vez de complejo: sigue en local y no propone gastar.
  - Garantía comprobada en ambos: **ninguna tarea sencilla se clasifica como compleja** (nunca se propone pagar por un saludo o una acción simple).
- **Probado en la app (Linux, modelos simulados):** saludo → local sin preguntar; comparación compleja → tarjeta con motivo y costo estimado; «Usar ChatGPT» → la petición fue al proveedor externo y el gasto quedó registrado; mensaje complejo con contraseña → local, sin tarjeta y sin gasto; Ajustes muestra el gasto del mes.
- **Pendiente en Windows real:** que el costo estimado se parezca al real con respuestas de verdad (el simulador declara un uso inventado), y que la tarjeta resulte útil y no molesta en el uso diario.
