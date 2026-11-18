# Arquitectura de CANELA AI (v0.2)

Este documento describe cómo está construida Canela y cómo se incorporarán las funciones de la Fase 3. Las secciones marcadas **[DISEÑO]** son planes: **todavía no están implementadas**.

## Visión general

```
┌──────────────────────────────── Proceso nativo (Rust, Tauri 2) ────────────────────────────────┐
│ lib.rs          arranque, bandeja, atajos globales, complementos                              │
│ windows.rs      ventana "pet" (transparente) y ventana "panel"                                 │
│ fullscreen.rs   [Windows] detecta juegos/videos a pantalla completa → oculta a Canela          │
│ commands.rs     comandos IPC: ajustes, claves, chat, confirmaciones, trabajos, carpetas        │
│ data_cmds.rs    comandos de memoria, tareas, actividad y conversaciones                        │
│ db.rs           SQLite local: memories, tasks, activity, conversations                         │
│ secrets.rs      claves API en el Administrador de credenciales de Windows                      │
│ reminders.rs    recordatorios (cada 30 s) + saludo al iniciar                                  │
│ jobs.rs         cola de trabajos en segundo plano (2 a la vez, cancelables)                    │
│ ai/             capa de IA                                                                     │
│   mod.rs          tipos comunes, prompt de Canela, run_turn (una petición en streaming)        │
│   agent.rs        bucle del agente: modelo → herramientas (con permiso) → modelo …             │
│   anthropic.rs    Claude: Messages API, razonamiento adaptativo, fallback, herramientas        │
│   openai.rs       ChatGPT: Chat Completions con funciones                                      │
│   sse.rs          parser de streaming                                                          │
│ tools/          herramientas                                                                   │
│   mod.rs          definiciones, validación (prepare), permisos y ejecución (run)               │
│   files.rs        archivos limitados a carpetas autorizadas                                    │
│   apps.rs         abrir apps del menú Inicio, webs http(s) y rutas                             │
└───────────────▲────────────────────────────────────────────────────▲────────────────────────────┘
                │ invoke / Channel<StreamEvent>                       │ eventos
┌───────────────┴──────── Ventana "pet" ─────────┐   ┌───────────────┴──── Ventana "panel" ────────┐
│ pet/brain.ts       máquina de estados (pura)    │   │ Chat: streaming, herramientas, permisos,    │
│ pet/sprites.ts     motor de spritesheets        │   │       trabajos, historial                   │
│ pet/controller.ts  movimiento, arrastre,        │   │ Tareas: lista, fechas, recordatorios        │
│                    clic-a-través, menú, render  │   │ Ajustes: IA, Canela, inicio, memoria,       │
│                    bajo demanda                 │   │          permisos y carpetas                │
└─────────────────────────────────────────────────┘   └─────────────────────────────────────────────┘
```

## Principios

1. **La clave API nunca llega a la interfaz.** Todas las llamadas a la IA las hace Rust.
2. **Historial de sólo-anexar.** Cada respuesta guarda su transcripción original (razonamiento firmado, llamadas a herramientas y resultados) y se reenvía tal cual. Esto lo exigen los modelos actuales de Claude y además mejora la caché.
3. **El modelo no ejecuta nada directamente.** Pide una herramienta; Rust valida los parámetros, decide el nivel de permiso y sólo entonces actúa.
4. **Nada se borra sin preguntarte.** No existe herramienta de borrado de archivos ni de comandos de consola.
5. **Ligera.** Render sólo cuando cambia el fotograma, sondeos adaptativos, pausa cuando Canela está oculta, trabajos pesados fuera del hilo principal.

## Herramientas y permisos

| Herramienta | Qué hace | Permiso |
|---|---|---|
| `recordar` | Guarda un recuerdo (rechaza datos sensibles) | Automático |
| `buscar_recuerdos` / `buscar_conversaciones` | Consulta memoria / historial | Automático |
| `olvidar` | Borra un recuerdo | **Confirmación** |
| `crear_tarea` / `listar_tareas` / `actualizar_tarea` | Tareas y recordatorios | Automático |
| `eliminar_tarea` | Borra una tarea | **Confirmación** |
| `abrir_aplicacion` | Abre un acceso directo del menú Inicio o una app del sistema | Automático (configurable) |
| `abrir_web` | Abre una URL http(s) o una búsqueda de Google | **Confirmación** (configurable) |
| `listar_carpeta` / `buscar_archivos` / `archivos_recientes` / `leer_archivo` | Lectura dentro de carpetas autorizadas | Automático |
| `abrir_ruta` | Abre carpeta o documento (nunca ejecutables) | Automático (configurable) |
| `crear_carpeta` | Crea una carpeta | **Confirmación** |
| `escribir_archivo` | Crea o modifica un archivo; muestra los cambios; guarda copia | **Confirmación** |
| `organizar_carpeta` | Mueve archivos sueltos a subcarpetas por tipo (no borra) | **Confirmación** con plan |
| `analizar_proyecto` | Trabajo en segundo plano: estructura + archivos clave → informe | Automático |

Carpetas autorizadas por defecto: Documentos, Descargas y Escritorio (editable en Ajustes › Permisos). Las rutas se resuelven a su forma canónica, así que `..` o accesos directos no permiten salir de ellas.

Para añadir una herramienta: define su esquema en `tools::definitions()`, su validación en `prepare()` (aquí decides el permiso) y su ejecución en `run()`, y añade una prueba.

## Memoria

- Tablas: `memories(kind, content, source)`, `tasks(title, priority, due, remind_at, …)`, `activity`, `conversations`.
- Al empezar una conversación se toma una "foto" de la memoria (40 recuerdos más recientes) y la fecha, y se incluye en el prompt durante toda la conversación. Lo nuevo que se recuerda a mitad de conversación lo conoce Canela por el resultado de la herramienta.
- Filtro de datos sensibles (`db::looks_sensitive`): contraseñas, tarjetas, claves de API y similares no se guardan automáticamente.

## Multitarea

- Varias respuestas pueden estar en curso (cada una con su id y cancelación).
- `jobs.rs`: cola con semáforo (2 trabajos simultáneos, 8 activos como máximo), estados `queued/running/done/failed/cancelled`, progreso y cancelación; eventos `jobs-changed` y `job-finished` hacia la interfaz.
- El chat y las animaciones nunca esperan a un trabajo.

## [DISEÑO] Voz — Fase 3 (no implementada)

- **Pulsar para hablar** (botón y atajo `Ctrl+Alt+V`): el micrófono sólo se abre mientras está activo y se muestra un indicador visible en el panel y en Canela (animación `listen`).
- **Reconocimiento:** API oficial de transcripción del proveedor elegido (OpenAI) o el reconocimiento de voz de Windows (`Windows.Media.SpeechRecognition`) para modo sin conexión. Captura de audio en Rust con `cpal`.
- **Respuestas habladas:** voces de Windows (SAPI/`Windows.Media.SpeechSynthesis`) o la API de voz del proveedor; Canela usa la animación `talk` sincronizada.
- **"Hola, Canela":** detector de palabra clave local y opcional (desactivado por defecto), que sólo escucha esa frase en el equipo y nunca envía audio hasta activarse.
- Ajustes: micrófono a usar, voz, velocidad.

## [DISEÑO] Visión de pantalla — Fase 3 (no implementada)

- Captura **sólo bajo petición explícita** (botón "Mostrar mi pantalla" o atajo), con vista previa y opción de recortar antes de enviarla.
- Captura en Rust (`xcap` o Windows Graphics Capture), imagen reducida a los límites del modelo y enviada como contenido de imagen a Claude o ChatGPT.
- Nunca capturas automáticas ni periódicas.

## [DISEÑO] Integraciones — Fase 3 (no implementadas)

| Servicio | Enfoque previsto |
|---|---|
| GitHub | OAuth (Device Flow) + API REST oficial; herramientas de lectura (repos, issues, PRs) automáticas, escritura con confirmación |
| Google Calendar / Gmail | OAuth 2.0 con PKCE y redirección a `127.0.0.1`; ámbitos mínimos (sólo lectura al principio); enviar correo siempre con confirmación |
| OpenAI / Anthropic | Ya integrados (Fases 1–2) |

Los tokens se guardarían en el Administrador de credenciales igual que las claves API, y cada integración tendría su interruptor en Ajustes.

## Rendimiento

- Ventana de la mascota pequeña que se mueve (no una capa transparente a pantalla completa).
- Dibujo sólo al cambiar de fotograma; temporizador de movimiento sólo mientras camina; sondeo del cursor cada 50 ms cerca de Canela y cada 300 ms lejos; todo en pausa cuando está oculta.
- "Animaciones reducidas": mitad de velocidad y sin pasear.
- Pantalla completa (Windows): Canela se oculta sola.
- Varios monitores: Canela pasea dentro del monitor en el que está y recalcula el área al arrastrarla a otro.
