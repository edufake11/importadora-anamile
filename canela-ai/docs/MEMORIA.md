# Memoria de tu PC y Canela (v0.5)

Este documento explica lo que viste en tu equipo (RAM al 99–100 %, disco C: al 100 %, memoria comprometida de 49 GB) y cómo usar Canela v0.5 para averiguar qué la causa. **Nada de lo que se describe aquí se ha comprobado todavía en tu PC**: son explicaciones técnicas y las mediciones que se hicieron en el entorno de pruebas (Linux).

## 1. Cuatro medidas distintas (no son lo mismo)

| Medida | Qué es | Dónde la ves |
|---|---|---|
| **RAM física en uso** | Memoria real ocupada ahora (tus 16 GB). | Administrador de tareas → Rendimiento → Memoria → «En uso» |
| **Memoria comprometida** (*commit*) | Memoria que Windows **prometió** a los programas: RAM + archivo de paginación. Puede superar la RAM física. Si llega al límite, los programas fallan. | Administrador de tareas → Memoria → «Confirmada» (X/Y GB) |
| **Memoria privada** de un proceso | Lo que sólo ese programa ha reservado. Cuenta para la comprometida, esté en RAM o en el disco (paginación). | Administrador de tareas → Detalles → columna «Tamaño de confirmación» |
| **Conjunto de trabajo** (*working set*) | La parte de un programa que está **ahora** en la RAM física. | Detalles → «Memoria (conjunto de trabajo activo)» |
| **VRAM** | Memoria de la tarjeta gráfica (tus 16 GB de la RTX 5060 Ti). | Rendimiento → GPU → «Memoria de GPU dedicada» |

Canela v0.5 muestra todas en **Ajustes → Rendimiento y memoria**.

## 2. Lo que probablemente pasó en tu equipo

1. **Qwen3 8B «en la GPU» también ocupa memoria comprometida.** `ollama ps` decía 100 % GPU y ≈ 7,5 GB, pero `llama-server` tenía ≈ 8,3 GB de memoria **privada**. Al hacer `ollama stop` la memoria comprometida bajó de 49 a 22 GB (−27 GB). Dos efectos conocidos en Windows lo explican:
   - el proceso del modelo guarda en RAM una copia de trabajo/búferes del modelo (su memoria privada), y
   - con los controladores de Windows (WDDM), las reservas de memoria de la GPU **también se cargan a la memoria comprometida** del sistema (para poder moverlas a RAM si la GPU se llena). Por eso la comprometida sube mucho más que la RAM.
   El contexto de 16 384 tokens añade ≈ 2,4 GB más de caché; con 8 192 (modo Ahorro) es la mitad.
2. **Cuando la RAM física se llena, Windows usa el disco** (archivo de paginación): de ahí el disco C: al 100 % y que todo vaya lento. Es la consecuencia, no la causa.
3. **La memoria comprometida volvió a subir con `ollama ps` vacío**, así que **había otro consumidor**. El candidato más claro es el **proceso de Python de ≈ 7,3 GB** que apareció y desapareció: **Canela no usa Python** (está hecha en Rust y su interfaz es WebView2), y Ollama tampoco. Puede ser un script, un entorno de IA (Jupyter, Stable Diffusion, un instalador…) u otra aplicación. Otros sospechosos habituales: navegador con muchas pestañas, juegos o programas con aceleración gráfica (también cargan memoria comprometida por WDDM), antivirus analizando archivos grandes (como los modelos de 5 GB) y fugas de controladores.
4. **`Get-Counter` falló con `c0000bb8`**: es un problema de los contadores de rendimiento de Windows (su registro está dañado o desactivado), no de la memoria ni de Canela. Canela **no los usa**: lee la memoria con `GetPerformanceInfo`. Si quieres repararlos, Microsoft documenta el comando `lodctr /R` en una consola de administrador; Canela nunca lo ejecuta por ti.

## 3. Cómo encontrar al culpable con Canela

1. Deja el **registro activado** (Ajustes → Rendimiento y memoria → Registro de eventos → «Guardar el registro»).
2. Usa el PC normalmente. Canela mide cada 60 s y apunta con la hora:
   - **aparece**: un programa nuevo con ≥ 1 GB de memoria privada (PID y ruta),
   - **crece**: un programa que sube ≥ 512 MB de golpe o cada +1 GB acumulado,
   - **desaparece**: un programa de ≥ 1 GB que terminó,
   - **RAM baja** (< 1,5 GB libres o < 10 %), **memoria comprometida alta** (≥ 90 % del límite),
   - **la memoria comprometida subió ≥ 3 GB en 10 min sin ningún modelo cargado**, con la lista de quién creció.
3. Cuando notes la saturación, abre el **Registro de eventos** o pulsa **Exportar informe** (JSON con todas las mediciones). Pasa el ratón sobre un programa para ver su ruta.
4. Los archivos diarios quedan en `%APPDATA%\com.canela.ai\diagnostico` (se borran solos a los 7 días).

## 4. Lo que hace Canela para no saturar tu PC

- **Descarga Qwen3 tras 2 min sin uso** (`keep_alive`); en *Ahorro*, a los 30 s y con contexto 8 192.
- **Nunca lo descarga a mitad de una respuesta** ni mientras ejecuta herramientas (plazo renovado sólo si el modelo sigue cargado).
- **Botón «Liberar memoria de IA»** (Qwen3 + modelo de voz).
- El reconocimiento de voz se carga sólo al usarlo y se libera solo; **Piper no deja memoria** (se ejecuta por frase).
- **Nunca** cierra programas ajenos ni toca el archivo de paginación.

## 5. Mediciones en el entorno de pruebas (Linux, no tu PC)

- **Canela en reposo (v0.4, 15 min)**: proceso principal estable (175 MB residentes, 76 MB privados, −0,3 MB en 15 min); ventanas web 523 → 540 MB (+17 MB, se va estabilizando). Total ≈ 0,7 GB residente.
- **whisper small cargado**: +≈ 250 MB mientras está en memoria; al liberarlo el proceso vuelve cerca del inicio.
- **whisper tiny («Hola, Canela»)**: ≈ 0,2–0,3 s por ráfaga de voz en una CPU de 4 núcleos lentos.
- Las cifras de Windows (WebView2) serán distintas: compruébalas en Ajustes → Rendimiento y memoria («Canela (con sus ventanas)»).
