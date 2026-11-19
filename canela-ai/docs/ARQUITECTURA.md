# Arquitectura de CANELA AI (v0.4)

Este documento describe cómo está construida Canela. Las secciones marcadas **[DISEÑO]** son planes: **todavía no están implementadas**. La v0.4 se verificó en Linux (Xvfb, micrófono virtual, Ollama/OpenAI/Claude simulados) y en el CI de Windows; falta la prueba en un equipo Windows 11 real y con un Ollama real.

## Visión general

```
┌──────────────────────────────── Proceso nativo (Rust, Tauri 2) ────────────────────────────────┐
│ lib.rs          arranque, bandeja, atajos globales, complementos                              │
│ windows.rs      ventana "pet" (transparente) y ventana "panel"                                 │
│ fullscreen.rs   [Windows] detecta juegos/videos a pantalla completa → oculta a Canela          │
│ commands.rs     comandos IPC: ajustes, claves, chat, confirmaciones, trabajos, carpetas        │
│ data_cmds.rs    comandos de memoria, tareas, actividad y conversaciones                        │
│ db.rs           SQLite local (migraciones v1–v3): memories (+caducidad), tasks, activity,     │
│                 conversations, doc_files + doc_chunks (FTS5), routines                         │
│ voice/          vad.rs (fin de frase), audio.rs (cpal, remuestreo, reproducción),              │
│                 mod.rs (grabar → WAV en memoria → transcribir; voz de OpenAI)                  │
│ vision.rs       capturas (xcap), redimensionado, ventanas sensibles excluidas                  │
│ vision_cmds.rs  ocultar mascota/panel, capturar, vista previa, recorte, adjuntos               │
│ media.rs        teclas multimedia (lista fija) · system.rs  estado del equipo (sysinfo)       │
│ docs.rs         texto de PDF (pdf-extract), Word (.docx/zip), partes de 12 000 caracteres      │
│ model_cmds.rs   capacidades del modelo (API de modelos de Anthropic), resúmenes               │
│ secrets.rs      claves API en el Administrador de credenciales de Windows                      │
│ reminders.rs    recordatorios (cada 30 s) + saludo al iniciar                                  │
│ jobs.rs         cola de trabajos en segundo plano (2 a la vez, cancelables)                    │
│ ai/             capa de IA                                                                     │
│   mod.rs          tipos comunes, prompt de Canela, run_turn (una petición en streaming)        │
│   agent.rs        bucle del agente: modelo → herramientas (con permiso) → modelo …             │
│   anthropic.rs    Claude: Messages API, razonamiento adaptativo, fallback, herramientas        │
│   openai.rs       ChatGPT: Chat Completions con funciones                                      │
│   ollama.rs       Ollama local: /api/chat NDJSON, think oculto, num_ctx, sólo 127.0.0.1        │
│   sse.rs          parser de streaming                                                          │
│   pricing.rs      uso de tokens → costo aproximado; ocultar claves en mensajes de error        │
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
6. **Lo que se lee no da órdenes.** Archivos, documentos, imágenes y capturas se devuelven al modelo envueltos en `<<<CONTENIDO EXTERNO …>>>`. A partir de ese momento el turno queda "contaminado": cualquier herramienta que no sea de sólo lectura pasa a pedir confirmación, con el aviso «⚠ Propuesto después de leer contenido externo» (`agent.rs`, prueba `injected_instructions_require_confirmation`).
7. **Micrófono y pantalla bajo demanda.** Nada graba ni captura en segundo plano; el audio sólo vive en memoria y las capturas se descartan si no las envías.

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
| `listar_carpeta` / `buscar_archivos` / `archivos_recientes` | Nombres de archivos dentro de carpetas autorizadas | Automático |
| `leer_archivo` | Envía el contenido de un archivo a la IA, como contenido externo | **Confirmación** (por defecto; desactivable en Ajustes) |
| `abrir_ruta` | Abre carpeta o documento (nunca ejecutables) | Automático (configurable) |
| `crear_carpeta` | Crea una carpeta | **Confirmación** |
| `escribir_archivo` | Crea o modifica un archivo; muestra los cambios; guarda copia | **Confirmación** |
| `organizar_carpeta` | Mueve archivos sueltos a subcarpetas por tipo (no borra) | **Confirmación** con plan |
| `analizar_proyecto` | Trabajo en segundo plano: estructura + archivos clave → informe | Automático |
| `leer_documento` | PDF/Word/texto por partes (extraído en tu equipo), como contenido externo | **Confirmación** (por defecto; desactivable en Ajustes) |
| `ver_imagen` | Envía una imagen de las carpetas autorizadas al modelo | **Confirmación** |
| `buscar_en_documentos` / `indexar_documentos` | Índice local FTS5 dentro de carpetas autorizadas | Automático |
| `abrir_con` | Abre ruta con VS Code, Explorador o Bloc de notas | Automático (configurable) |
| `ver_pantalla` | Captura pedida por el modelo; vista previa en la tarjeta de permiso | **Confirmación** siempre |
| `ejecutar_plan` / `ejecutar_rutina` | Varios pasos con herramientas existentes (máx. 10) | **Confirmación** del plan completo + re-confirmación de pasos delicados |
| `guardar_rutina` / `listar_rutinas` | Rutinas con nombre en SQLite | Automático |
| `controlar_multimedia` | Teclas multimedia fijas (0xAD–0xB3) con `SendInput`; volumen 1–25 pasos | Automático (configurable en Ajustes) |
| `info_sistema` | `sysinfo`: SO, CPU, RAM, discos (sólo letra de unidad), batería, programas por memoria; sin usuario ni nombre de equipo | Automático (sólo lectura) |

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

## Cerebro local: Ollama (v0.4)

- **API nativa** `POST {127.0.0.1:11434}/api/chat` con `stream: true` (NDJSON, una línea JSON por trozo). No se usa la API compatible con OpenAI porque no permite `options.num_ctx` ni `think`.
- **Contexto**: `options.num_ctx` = 16 384 por defecto (4 096–131 072). Instrucciones + 31 herramientas ≈ 5 200 tokens (prueba `prompt_and_tools_fit_default_context`), así que el valor por defecto de Ollama (4 096) no alcanza. KV de qwen3:8b ≈ 144 KB/token → 16 384 ≈ 2,4 GB + 5,2 GB de pesos. `keep_alive: 30m`.
- **Razonamiento**: `think` = ajuste «Pensar más» (falso por defecto). `message.thinking` nunca se muestra ni se reenvía (sólo emite el estado «pensando» de Canela). `ThinkFilter` quita además `<think>…</think>` del texto aunque llegue partido entre trozos.
- **Herramientas**: mismas definiciones que Claude/ChatGPT; `message.tool_calls[].function.arguments` llega como objeto; ids generados si faltan; resultados como `{role:"tool", tool_name, content}`. Pasan por el mismo `prepare → permiso → run`, la defensa de contenido externo y las confirmaciones.
- **Compatibilidad**: si Ollama responde 400 «does not support thinking/tools», `ai::complete` lo recuerda para ese modelo y reintenta sin el campo (aviso si el modelo no admite herramientas).
- **Privacidad**: `loopback_url` sólo acepta `127.0.0.1`, `localhost` o `::1` (también al cargar ajustes). Sin cabecera de autenticación. Con Ollama, `cloud_voice_allowed` bloquea la transcripción y la voz de OpenAI salvo `ollama_cloud_voice`. Resúmenes y análisis de proyectos usan el mismo proveedor local.
- **Estado**: `ollama_status` consulta `/api/version`, `/api/tags` (modelos y tamaños) y `/api/show` (capacidades: tools, thinking, vision; longitud de contexto). Nunca descarga modelos.
- **Errores claros**: conexión rechazada → «No encuentro Ollama…»; 404/«not found» → `ollama pull <modelo>`; falta de memoria → bajar contexto o cerrar programas; error a mitad del stream (`{"error": …}`) → se muestra.
- **Costo**: 0 (tabla de precios); la interfaz muestra «local».
- **Imágenes**: sólo modelos de visión (llava, *-vl, gemma3…); con qwen3:8b las capturas se rechazan con un mensaje claro.

## Voz (v0.3)

- **Pulsar para hablar**: botón 🎙 o `Ctrl+Alt+V` (funciona con el panel cerrado). Requiere aceptar el consentimiento en Ajustes (`voice_consent`) y la clave de OpenAI.
- **Captura**: `cpal` en un hilo propio → mono → remuestreo a 16 kHz → `vad.rs` (detector por energía con piso de ruido adaptativo: calibra 240 ms, fin de frase tras 900 ms de silencio, abandona a los 8 s sin voz, máximo 45 s). Evento `voice-state` con nivel para el indicador.
- **Transcripción**: WAV construido en memoria → `POST /v1/audio/transcriptions` (modelo `gpt-transcribe`, idioma `es`). El audio nunca se escribe en disco.
- **Respuesta hablada**: voces de Windows mediante `speechSynthesis` de WebView2 (gratis, sin red) o `POST /v1/audio/speech` (`gpt-4o-mini-tts`, PCM 24 kHz reproducido con `cpal`). El texto se va diciendo por frases mientras llega; vigilante contra bloqueos; interrumpible.
- **Canela**: `listen` mientras graba, `curious` al transcribir, `think`, `talk` mientras habla, `sad` si falla.
- **[DISEÑO] «Hola, Canela»**: detector de palabra clave local (p. ej. openWakeWord, ~40 MB). Desactivado en la interfaz; sólo se implementará con descarga autorizada mostrando el tamaño.
- **[DISEÑO] Sin conexión**: whisper.cpp con CUDA (modelo *small* ≈ 0,5 GB o *large-v3-turbo* ≈ 1,6 GB) y Piper para voz. Con una RTX 5060 Ti 16 GB la transcripción local sería más rápida que la API, pero exige descargas grandes; no se descargan automáticamente.

## Visión de pantalla (v0.3)

- `vision.rs`: `xcap` lista monitores y ventanas (filtra títulos de contraseñas/bancos, `is_sensitive`) y captura un monitor, una ventana o la ventana activa.
- `vision_cmds.rs`: oculta mascota y panel ~280 ms, captura, restaura; la imagen queda en memoria con un id; la interfaz recibe una vista previa reducida. `screen_finalize` aplica el recorte y la reduce a ≤1568 px / ≤1,15 MP (JPEG 82); `screen_discard` la borra.
- Envío como bloque `image` (Claude) o `image_url` (OpenAI). Si el modelo no acepta imágenes se avisa antes.
- Nunca capturas automáticas ni periódicas; `ver_pantalla` (cuando lo pide el modelo) siempre muestra la imagen en la tarjeta de permiso.

## Documentos (v0.3)

- `docs.rs`: texto plano/código ≤5 MB; PDF y Word ≤40 MB. PDF con `pdf-extract` dentro de `catch_unwind`; si hay menos de 5 caracteres reales por página se considera escaneado y se avisa (no hay OCR).
- Partes de 12 000 caracteres (`leer_documento` con `parte`); los adjuntos del chat se recortan a 60 000 caracteres.
- Índice FTS5 (`unicode61 remove_diacritics 2`) con fragmentos y `bm25`; sólo se reindexa lo que cambió y se olvida lo que ya no existe. Se borra desde Ajustes.

## Contexto y modelos (v0.3)

- Conversaciones largas (> 350 000 caracteres) se compactan: el modelo resume lo antiguo y se conserva lo reciente.
- Recuerdos temporales con `expires_at`; se purgan al arrancar.
- `model_cmds.rs`: `GET /v1/models/{id}` de Anthropic para saber si admite imágenes, razonamiento adaptativo y esfuerzo, y sus límites de tokens; para OpenAI se comprueba que el modelo exista y se usa una tabla.
- `pricing.rs`: costo aproximado por respuesta (entrada, salida, caché) mostrado en el chat. Los errores de la API se muestran en lenguaje sencillo y pasan por `redact_secrets` para que nunca aparezca una clave.

## Automatizaciones (v0.3)

- `ejecutar_plan {titulo, pasos[]}`: `validate_steps` comprueba que cada herramienta exista, que no sea otra automatización y que tenga sus parámetros obligatorios. Máximo 10 pasos.
- El usuario ve el plan numerado y lo aprueba una vez. Los pasos de `NEEDS_OWN_CONFIRM` (borrar tarea/recuerdo, escribir archivo, organizar carpeta, capturas, imágenes) se vuelven a confirmar; las demás reglas de permiso (incluida la de contenido externo) siguen aplicando.
- 120 s por paso, 10 min en total, parada al primer error o rechazo, botón ■ para cancelar; cada plan queda en `activity`.
- Rutinas: tabla `routines(name, steps)`; `ejecutar_rutina` vuelve a validar los pasos y pasa por la misma aprobación.

## [DISEÑO] Integraciones — Fase 3 (no implementadas)

| Servicio | Enfoque previsto |
|---|---|
| GitHub | OAuth (Device Flow) + API REST oficial; herramientas de lectura (repos, issues, PRs) automáticas, escritura con confirmación |
| Google Calendar / Gmail | OAuth 2.0 con PKCE y redirección a `127.0.0.1`; ámbitos mínimos (sólo lectura al principio); enviar correo siempre con confirmación |
| OpenAI / Anthropic | Ya integrados (chat, imágenes, transcripción y voz) |

Los tokens se guardarían en el Administrador de credenciales igual que las claves API, y cada integración tendría su interruptor en Ajustes.

## Rendimiento

- Ventana de la mascota pequeña que se mueve (no una capa transparente a pantalla completa).
- Dibujo sólo al cambiar de fotograma; temporizador de movimiento sólo mientras camina; sondeo del cursor cada 50 ms cerca de Canela y cada 300 ms lejos; todo en pausa cuando está oculta.
- "Animaciones reducidas": mitad de velocidad y sin pasear.
- Pantalla completa (Windows): Canela se oculta sola.
- Varios monitores: Canela pasea dentro del monitor en el que está y recalcula el área al arrastrarla a otro.
