# 🐾 CANELA AI — Fase 1 (MVP)

Mascota virtual y asistente inteligente de escritorio para Windows 11. Canela es una perrita salchicha color chocolate, con audífonos violetas y pañuelo morado, que vive en tu escritorio, pasea, duerme, reacciona a tus clics y conversa contigo usando **Claude (Anthropic)** o **ChatGPT (OpenAI)**.

> Estado: **Fase 1 completa**. La memoria persistente, el control de Windows, la voz y la visión de pantalla son las fases 2 y 3 y **todavía no están implementadas** (ver [Limitaciones](#limitaciones-actuales)).

---

## Arquitectura

```
┌──────────────────────────── Proceso Tauri (Rust) ────────────────────────────┐
│ lib.rs        arranque, bandeja del sistema, atajos globales                  │
│ windows.rs    ventana "pet" (transparente) y ventana "panel" (asistente)      │
│ commands.rs   comandos IPC: ajustes, claves, chat (streaming + cancelación)   │
│ settings.rs   ajustes no secretos → %APPDATA%\com.canela.ai\settings.json     │
│ secrets.rs    claves API → Administrador de credenciales de Windows           │
│ ai/           capa de IA desacoplada del proveedor                            │
│   ├─ mod.rs        trait ProviderStream, StreamEvent, prompt de Canela        │
│   ├─ sse.rs        parser SSE incremental (UTF-8 seguro)                      │
│   ├─ anthropic.rs  Messages API (streaming, razonamiento adaptativo, fallback)│
│   └─ openai.rs     Chat Completions API (streaming)                           │
└───────────────▲──────────────────────────────────────────▲────────────────────┘
                │ invoke / Channel<StreamEvent>            │ eventos (emitTo)
┌───────────────┴────────── Ventana "pet" ─────┐  ┌────────┴──── Ventana "panel" ─────┐
│ React + TS                                    │  │ React + TS (Modo asistente)        │
│ pet/brain.ts      máquina de estados (pura)   │  │ Chat (streaming, adjuntos, Stop)   │
│ pet/sprites.ts    motor de spritesheets       │  │ Ajustes (proveedor, modelo, claves)│
│ pet/controller.ts movimiento, arrastre,       │  │ Acerca de (estado real del proyecto│
│                   clic-a-través, menú nativo  │  │ y privacidad)                      │
└───────────────────────────────────────────────┘  └────────────────────────────────────┘
```

Decisiones clave:

| Tema | Decisión | Motivo |
|---|---|---|
| Escritorio | **Tauri 2** | Instalador de pocos MB y bajo consumo de memoria: usa el WebView2 que ya trae Windows 11 en lugar de empaquetar un navegador (Electron). |
| Interfaz | **React 19 + TypeScript + Vite** | Mantenible, tipado y rápido de compilar. |
| Llamadas a la IA | **En Rust**, no en la webview | La clave API nunca llega al JavaScript; la webview sólo recibe texto. |
| Claves API | **Llavero del SO** (`keyring`) | En Windows: Administrador de credenciales (DPAPI). Nada en archivos ni código. |
| Streaming | `tauri::ipc::Channel` + SSE | Texto en tiempo real y cancelación (botón *Detener*). |
| Nuevos modelos | trait `ProviderStream` | Añadir un proveedor = un archivo en `ai/` + una variante en `ProviderId`. |
| Mascota | Ventana pequeña que **se mueve** (no una capa a pantalla completa) | Menos consumo y no bloquea el escritorio. |
| Clic a través | Hit-test por píxel cada 50 ms + `setIgnoreCursorEvents` | Las zonas transparentes dejan pasar el ratón a las demás apps. |
| Animación | Spritesheet PNG + manifiesto JSON + máquina de estados pura | El arte se puede reemplazar sin tocar código. |
| Diseño | Interfaz **monocroma** (negro, blanco y grises), dos pestañas (Chat · Ajustes), opciones avanzadas plegadas | Lectura clara y sin ruido visual; el color queda reservado a Canela (audífonos y pañuelo). |
| Permisos | *Capabilities* de Tauri por ventana | Cada ventana sólo tiene las APIs que necesita; CSP estricta. |

### Detalles de la integración con Claude

- `POST https://api.anthropic.com/v1/messages` con `anthropic-version: 2026-10-21` (no existe SDK oficial de Anthropic para Rust, así que se usa la API REST documentada).
- Modelo por defecto: `claude-opus-5-5`, con **razonamiento adaptativo** y `output_config.effort` configurable (por defecto `medium`).
- **Fallback del servidor activado** (`fallbacks: "default"` + beta `server-side-fallback-2026-07-01`): si los clasificadores de seguridad rechazan una petición legítima, la API reintenta con un modelo de respaldo dentro de la misma llamada. Si alguna vez quieres desactivarlo, elimina `fallbacks` en `ai/anthropic.rs`.
- Si aun así hay rechazo (`stop_reason: "refusal"`), Canela lo explica en el chat en vez de mostrar una respuesta vacía.
- Caché de prompt automática y **historial de sólo-anexar**: los bloques de razonamiento firmados se devuelven tal cual en cada turno (requisito de los modelos actuales) y el prompt de sistema no cambia dentro de una conversación.

---

## Estructura de carpetas

```
canela-ai/
├── package.json · vite.config.ts · tsconfig.json · index.html
├── scripts/
│   └── generate-sprites.mjs      # genera el spritesheet provisional e iconos
├── assets/canela-icon.png        # icono fuente 1024×1024
├── public/sprites/
│   ├── canela.png                # spritesheet (13 animaciones)
│   ├── canela.json               # manifiesto de animaciones
│   └── canela-face.png           # cara para el panel
├── src/
│   ├── main.tsx                  # monta PetApp o PanelApp según la ventana
│   ├── lib/ipc.ts                # comandos y eventos tipados
│   ├── pet/                      # mascota: brain, sprites, controller, PetApp
│   ├── panel/                    # asistente: Chat, SettingsView, markdown, chatModel
│   └── styles/theme.css
├── src-tauri/
│   ├── Cargo.toml · build.rs · tauri.conf.json
│   ├── capabilities/{pet,panel}.json
│   ├── icons/                    # generados con `tauri icon`
│   └── src/ (main.rs, lib.rs, windows.rs, commands.rs, settings.rs, secrets.rs, ai/)
└── docs/SPRITES.md               # cómo crear e integrar el arte final
```

---

## Instalación en Windows 11

### 1. Requisitos (una sola vez)

Abre **PowerShell** y ejecuta:

```powershell
# Herramientas de compilación de C++ (necesarias para Rust en Windows)
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# Rust (toolchain stable-msvc)
winget install Rustlang.Rustup
rustup default stable-msvc

# Node.js LTS (20 o superior)
winget install OpenJS.NodeJS.LTS
```

WebView2 ya viene incluido en Windows 11. Cierra y vuelve a abrir PowerShell tras instalar.

### 2. Descargar y ejecutar en modo desarrollo

```powershell
git clone https://github.com/edufake11/importadora-anamile.git
cd importadora-anamile
git checkout claude/adoring-hamilton-3ish8u
cd canela-ai
npm install
npm run tauri dev
```

La primera compilación de Rust tarda varios minutos; las siguientes son rápidas. Canela aparecerá sobre la barra de tareas, abajo a la derecha.

### 3. Configurar la IA

1. Doble clic sobre Canela (o `Ctrl+Alt+Espacio`) → pestaña **Ajustes**.
2. Elige **Claude** o **ChatGPT**, pega tu clave de Anthropic (<https://console.anthropic.com/>) u OpenAI (<https://platform.openai.com/api-keys>) y pulsa **Guardar**. Se guarda en el Administrador de credenciales de Windows (puedes verla en *Panel de control → Administrador de credenciales → Credenciales de Windows → `com.canela.ai`*).
3. Pulsa **Guardar cambios**. El modelo y el nivel de razonamiento están en *Opciones avanzadas*.

### 4. Uso

| Acción | Resultado |
|---|---|
| Clic sobre Canela | Reacciona (se pone feliz, dice algo). 3 clics rápidos: celebra. Si duerme: se despierta. |
| Doble clic | Abre el asistente (Modo asistente). |
| Arrastrar | La mueves por el escritorio (animación de "colgando"). |
| Clic derecho | Menú: abrir asistente, dormir/despertar, sentarse, paseo, tamaño, ocultar, salir. |
| `Ctrl+Alt+C` | Ocultar / mostrar a Canela. |
| `Ctrl+Alt+Espacio` | Abrir / cerrar el asistente. |
| Icono en la bandeja | Clic: abrir asistente. Clic derecho: menú. |
| Cerrar el panel (✕) | Vuelve al **Modo mascota** (sólo Canela). La app sigue en la bandeja. |

---

## Generar el instalador

```powershell
cd canela-ai
npm run tauri build
```

Resultado:

- `src-tauri\target\release\bundle\nsis\Canela AI_0.1.0_x64-setup.exe` — instalador recomendado (por usuario, sin permisos de administrador).
- `src-tauri\target\release\bundle\msi\Canela AI_0.1.0_x64_es-ES.msi` — para despliegues con MSI (Tauri descarga WiX automáticamente).

El instalador no está firmado digitalmente, así que Windows SmartScreen mostrará un aviso ("Más información → Ejecutar de todas formas"). Para distribuirlo públicamente conviene un certificado de firma de código.

**Compilación automática en la nube:** el workflow `.github/workflows/canela-windows.yml` ejecuta las pruebas y genera ambos instaladores en un Windows de GitHub cada vez que cambia `canela-ai/`. Descárgalos desde la pestaña *Actions → Canela AI (Windows) → Artifacts*.

---

## Pruebas

```powershell
npm test                          # 21 pruebas de interfaz (máquina de estados, sprites, chat, markdown)
npm run typecheck                 # TypeScript estricto
cd src-tauri; cargo test          # 26 pruebas de Rust (SSE, Claude, OpenAI, ajustes, streaming HTTP de extremo a extremo)
```

Las pruebas de Rust incluyen un servidor HTTP local que imita la API con streaming troceado, errores 401, cortes de conexión y caracteres UTF-8 partidos, así que no necesitan red ni claves.

**Prueba manual recomendada en Windows 11** (lista de verificación):

1. Canela aparece sobre la barra de tareas con fondo transparente y sin sombra ni borde.
2. Puedes hacer clic en el escritorio/ventanas *a través* de las zonas vacías alrededor de Canela.
3. Pasea sola, se sienta, duerme tras ~2 min sin interacción (con "Z z z"), y se despierta con un clic.
4. Arrastrarla funciona; el menú de clic derecho funciona; el tamaño cambia desde el menú.
5. `Ctrl+Alt+C` la oculta/muestra; `Ctrl+Alt+Espacio` abre/cierra el panel.
6. Con una clave válida, la respuesta aparece en tiempo real; Canela "piensa" (…) y luego "habla" con una burbuja.
7. *Detener* corta la respuesta; *Nueva conversación* reinicia el contexto.
8. Una clave inválida muestra "La clave API … no es válida" y Canela se pone triste.

---

## Limitaciones actuales

Lo siguiente **no está implementado** en la Fase 1 (Canela lo sabe y lo dice si se lo pides):

- **Memoria persistente**: el contexto se recuerda sólo dentro de la conversación actual; se pierde al cerrar la app o pulsar *Nueva conversación*. (Fase 2: SQLite.)
- **Control de Windows**: no abre programas, no busca ni organiza archivos, no controla el navegador. (Fase 2, con permisos por herramienta y confirmación para acciones sensibles.)
- **Voz y visión de pantalla**: no usa micrófono ni capturas. (Fase 3.)
- **Multitarea**: hay una conversación a la vez (el núcleo ya admite varias respuestas simultáneas con ID y cancelación, base para la Fase 3).
- **Adjuntos**: sólo archivos de texto/código hasta 300 KB (no PDF, Word ni imágenes todavía).
- **Arte**: el spritesheet actual es **provisional**, generado por código a partir de la referencia. Ver [docs/SPRITES.md](docs/SPRITES.md) para crear el arte final y sustituirlo sin cambiar código.
- **Varios monitores**: Canela pasea dentro del monitor donde está; puedes arrastrarla a otro y seguirá allí.
- **Escritorios virtuales y apps a pantalla completa**: Canela está "siempre encima", pero los juegos en pantalla completa exclusiva pueden ocultarla.
- **Atajos**: si otra aplicación ya usa `Ctrl+Alt+C` o `Ctrl+Alt+Espacio`, ese atajo no se registra (la app sigue funcionando y lo indica en la consola). Aún no son configurables.
- **Modelos de OpenAI**: el modelo por defecto es `gpt-5`; cámbialo en Ajustes por cualquier modelo de chat disponible en tu cuenta.
- **Firma de código y actualizaciones automáticas**: no incluidas.
- Verificado en este entorno: compilación, pruebas automáticas y ejecución en Linux (WebKitGTK) con capturas de la mascota y el panel. La transparencia real depende del compositor de Windows (DWM) y debe confirmarse con la lista de verificación anterior en tu equipo.

## Próximas fases (resumen)

- **Fase 2 — Inteligencia y productividad**: memoria en SQLite (con pantalla para ver y borrar recuerdos), herramientas de Windows con *tool use* (abrir apps, buscar proyectos, organizar Descargas) bajo un sistema de permisos con confirmación explícita para acciones destructivas o que envían datos fuera, y gestor de tareas.
- **Fase 3 — Avanzado**: voz en español (pulsar para hablar, indicador visible de micrófono, "Hola, Canela" opcional), análisis de capturas de pantalla bajo petición explícita, multitarea con estado/pausa/cancelación, automatizaciones e integraciones, más animaciones y personalidad.
