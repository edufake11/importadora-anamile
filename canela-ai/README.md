# 🐾 CANELA AI — v0.2 (Fase 2)

Canela es una perrita salchicha en pixel art que vive en tu escritorio de Windows 11 y es tu asistente personal: conversa contigo (con Claude o ChatGPT), recuerda lo que le pidas, organiza tus tareas y recordatorios, abre aplicaciones y páginas, y trabaja con tus archivos **sólo en las carpetas que autorices y siempre pidiéndote permiso antes de cambiar algo**.

---

## Instalar en Windows 11 (sin conocimientos técnicos)

### 1. Descargar el instalador

1. Entra en <https://github.com/edufake11/importadora-anamile/actions/workflows/canela-windows.yml> (con tu cuenta de GitHub iniciada).
2. Abre la ejecución más reciente que tenga ✅ verde.
3. Abajo, en **Artifacts**, descarga **canela-ai-windows-instaladores** (es un .zip).
4. Descomprímelo. Dentro hay dos archivos; usa el que termina en **`-setup.exe`**.

### 2. Instalar

1. Doble clic en `Canela AI_0.2.0_x64-setup.exe`.
2. Windows mostrará "Windows protegió su PC" porque el instalador aún no tiene firma digital: pulsa **Más información → Ejecutar de todas formas**.
3. Sigue el asistente (no necesita permisos de administrador). Al terminar, Canela aparece sobre la barra de tareas, abajo a la derecha.

### 3. Configurar (una sola vez)

1. **Doble clic sobre Canela** → se abre su panel.
2. Ve a **Ajustes** → elige **Claude** o **ChatGPT**.
3. Pega tu clave API y pulsa **Guardar**:
   - Claude: crea una clave en <https://console.anthropic.com/settings/keys>
   - ChatGPT: crea una clave en <https://platform.openai.com/api-keys>
4. (Opcional) Escribe tu nombre, revisa las carpetas en **Permisos** y pulsa **Guardar cambios**.

¡Listo! Escríbele en **Chat**, por ejemplo: «¿Qué tengo que hacer hoy?», «Abre Visual Studio Code» o «Recuerda que estoy desarrollando una página web».

### Cómo se usa

| Quiero… | Haz esto |
|---|---|
| Hablar con Canela | Doble clic sobre ella, o `Ctrl + Alt + Espacio` |
| Esconderla o mostrarla | `Ctrl + Alt + C` |
| Ver sus opciones (dormir, tamaño, pasear, salir) | Clic derecho sobre ella |
| Moverla | Arrástrala con el ratón |
| Que reaccione | Un clic (3 clics rápidos: celebra) |
| Ver y editar mis tareas | Panel → **Tareas** |
| Ver o borrar lo que recuerda | Panel → **Ajustes → Memoria → Ver y editar recuerdos** |
| Cerrarla del todo | Clic derecho → *Salir*, o el icono de Canela junto al reloj |

Cuando Canela necesite hacer algo importante (crear o cambiar un archivo, organizar una carpeta, borrar una tarea o un recuerdo, abrir una página web), te mostrará una **tarjeta con lo que va a hacer** y esperará a que pulses **Permitir** o **No**.

---

## Qué incluye esta versión

**Canela (mascota)**
- 16 animaciones: caminar (izquierda y derecha), correr, sentarse, acostarse, dormir con «Zzz», despertarse y estirarse, mover la cola de felicidad, parpadear, inclinar la cabeza con curiosidad, reaccionar al clic, escuchar, pensar, hablar, celebrar, sorpresa y tristeza ante errores, y colgar mientras la arrastras.
- Descansa poco a poco cuando no la usas (sentada → acostada → dormida) y se pone curiosa cuando acercas el cursor.
- Ligera: dibuja sólo cuando cambia la imagen, se pausa cuando está oculta, opción de *animaciones reducidas* y se oculta sola con juegos o videos en pantalla completa.

**Asistente**
- Chat con Claude o ChatGPT, respuestas en tiempo real, adjuntar archivos de texto o código.
- **Memoria** local (SQLite): proyectos, preferencias, horarios, apps favoritas y datos que le pidas recordar. No guarda contraseñas ni datos bancarios.
- **Tareas**: prioridad, fecha, recordatorio con notificación de Windows, historial de actividad. También desde el chat («agrega estudiar programación a mis pendientes», «marca esa tarea como completada»).
- **Saludo al iniciar** con tus tareas del día y opción de abrir Canela al encender la computadora.
- **Computadora**: abrir aplicaciones del menú Inicio, abrir páginas o búsquedas web, listar, buscar y leer archivos, ver documentos recientes, crear carpetas, crear o modificar archivos (con vista previa de cambios y copia de seguridad), organizar Descargas por tipo.
- **Programación**: leer archivos de tus proyectos, explicar código y errores, proponer cambios que revisas antes de aplicar, y **analizar proyectos en segundo plano** mientras sigues conversando.
- **Historial de chats** opcional (desactivado por defecto).

## Lo que todavía NO hace

- **Voz** (micrófono y respuestas habladas) y **análisis de pantalla**: diseñados en [docs/ARQUITECTURA.md](docs/ARQUITECTURA.md), **no implementados**.
- **Google Calendar, Gmail y GitHub**: diseñados, **no implementados**.
- No ejecuta comandos de consola ni instala programas (decisión de seguridad).
- Las acciones en archivos sólo funcionan dentro de las carpetas autorizadas.
- Adjuntos: sólo texto o código (no PDF, Word ni imágenes todavía).
- El arte de Canela es provisional (generado por código); se puede reemplazar sin tocar el programa: [docs/SPRITES.md](docs/SPRITES.md).
- El instalador no está firmado digitalmente y no se actualiza solo.

---

## Qué probar cuando llegues a tu PC

1. **Aspecto**: Canela aparece abajo a la derecha, sin fondo ni bordes; puedes hacer clic en el escritorio "a través" del espacio vacío a su alrededor.
2. **Vida**: pasea, se sienta, se acuesta; tras unos minutos sin tocarla se acuesta y se duerme con «Zzz»; un clic la despierta y se estira; acerca el cursor y verás que inclina la cabeza.
3. **Atajos y menú**: `Ctrl+Alt+C`, `Ctrl+Alt+Espacio`, clic derecho → Tamaño.
4. **Chat**: con tu clave, pregunta algo; Canela "piensa" y luego "habla" con una burbuja.
5. **Tareas**: «agrega estudiar programación para hoy a las 6 pm con recordatorio a las 5:55» → aparece en **Tareas** y a la hora indicada sale una notificación de Windows.
6. **Memoria**: «recuerda que estoy desarrollando una página web» → luego, en una conversación nueva, «¿qué proyectos tengo?». Revísalo en Ajustes → Memoria.
7. **Apps**: «abre Visual Studio Code», «abre Chrome y busca información sobre React» (te pedirá permiso para la web).
8. **Archivos**: «muéstrame mis documentos recientes», «busca mi proyecto de React», «crea una carpeta para mi universidad» (pide permiso), «organiza mis descargas» (muestra el plan y pide permiso).
9. **Segundo plano**: «analiza mi proyecto <nombre>» y sigue chateando; al terminar aparece el informe y Canela celebra.
10. **Pantalla completa**: abre un video en pantalla completa: Canela debe ocultarse y volver al salir.
11. **Inicio con Windows**: activa la opción en Ajustes, reinicia y comprueba el saludo.

Si algo falla, cuéntame qué hiciste y qué viste (una captura ayuda mucho).

---

## Para desarrolladores

### Requisitos
- Windows 11 con WebView2 (incluido), [Rust](https://rustup.rs) (`stable-msvc`), Node.js 20+ y "Desktop development with C++" de Visual Studio Build Tools.

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
```

### Ejecutar y compilar

```powershell
git clone https://github.com/edufake11/importadora-anamile.git
cd importadora-anamile
git checkout claude/adoring-hamilton-3ish8u
cd canela-ai
npm install
npm run tauri dev        # modo desarrollo
npm run tauri build      # instaladores en src-tauri\target\release\bundle\{nsis,msi}
```

### Pruebas

```powershell
npm run typecheck        # TypeScript estricto
npm test                 # 30 pruebas de interfaz
cd src-tauri; cargo test # 54 pruebas de Rust (incluye el agente con Claude y ChatGPT simulados)
```

GitHub Actions (`.github/workflows/canela-windows.yml`) repite todo en Windows y publica los instaladores.

### Estructura

```
canela-ai/
├── scripts/generate-sprites.mjs   # genera el spritesheet provisional y los iconos
├── public/sprites/                # canela.png + canela.json (16 animaciones)
├── src/                           # interfaz React + TypeScript
│   ├── pet/                       # mascota: brain (estados), sprites, controller
│   └── panel/                     # Chat, Tareas, Ajustes, memoria
├── src-tauri/src/                 # núcleo Rust (ver docs/ARQUITECTURA.md)
└── docs/                          # ARQUITECTURA.md, SPRITES.md
```

Datos locales en Windows: ajustes en `%APPDATA%\com.canela.ai\settings.json`, base de datos y copias de seguridad de archivos en `%APPDATA%\com.canela.ai\` (`canela.db`, `copias\`). Claves API en el Administrador de credenciales (`com.canela.ai`).

### Integración con Claude

`POST /v1/messages` con `anthropic-version: 2026-10-21`; modelo por defecto `claude-opus-5-5` con razonamiento adaptativo y esfuerzo configurable; *fallback* del servidor activado (`fallbacks: "default"`) para que un rechazo de los filtros de seguridad se reintente con un modelo de respaldo; herramientas con `eager_input_streaming` y validación de la entrada antes de ejecutar nada; historial de sólo-anexar con los bloques de razonamiento firmados. Detalles en [docs/ARQUITECTURA.md](docs/ARQUITECTURA.md).
