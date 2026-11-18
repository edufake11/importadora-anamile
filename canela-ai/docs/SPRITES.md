# Sprites de Canela: cómo crear e integrar el arte final

El spritesheet incluido (`public/sprites/canela.png`) es **arte provisional** generado por `scripts/generate-sprites.mjs` a partir de formas geométricas. Sirve para que el MVP funcione; para la calidad de la imagen de referencia hay que dibujarlo a mano. El motor de animación no depende del arte: basta con sustituir dos archivos.

## Formato

- **Una fila por animación**, fotogramas de izquierda a derecha, todos del mismo tamaño.
- Tamaño de fotograma actual: **48 × 40 px** (puedes usar otro, p. ej. 64 × 48; se indica en el JSON).
- Personaje **mirando a la derecha** (el motor lo espeja para caminar a la izquierda).
- **Fondo transparente** (PNG RGBA). Sin antialiasing en los bordes: píxeles 100 % opacos o 100 % transparentes (el clic-a-través usa la opacidad).
- Las patas deben apoyarse en la misma fila en todos los fotogramas (`anchor.y` en el JSON).
- La cabeza está alrededor de la columna 35 (mirando a la derecha); la burbuja de diálogo apunta ahí. Si cambias mucho la composición, ajusta `headFrameX` en `src/pet/controller.ts`.

## Animaciones requeridas

| Fila | Nombre | Fotogramas | FPS | Bucle | Uso |
|---|---|---|---|---|---|
| 0 | `idle` | 4 | 4 | sí | De pie, cola, parpadeo |
| 1 | `walk` | 4 | 8 | sí | Caminar |
| 2 | `run` | 4 | 12 | sí | Correr |
| 3 | `sit` | 2 | 3 | sí | Sentada |
| 4 | `sleep` | 2 | 1.5 | sí | Dormida (respiración) |
| 5 | `wake` | 3 | 4 | no | Despertarse (termina sentada) |
| 6 | `listen` | 2 | 3 | sí | Escuchando (mientras escribes en el chat) |
| 7 | `talk` | 2 | 6 | sí | Hablando (mientras llega la respuesta) |
| 8 | `think` | 2 | 2 | sí | Pensando (esperando a la IA) |
| 9 | `celebrate` | 4 | 8 | sí | Saltar de alegría |
| 10 | `happy` | 2 | 6 | sí | Feliz (clic, respuesta terminada) |
| 11 | `sad` | 2 | 2 | sí | Triste (errores) |
| 12 | `dragged` | 2 | 6 | sí | Colgando mientras la arrastras |

Puedes añadir más fotogramas a cualquier animación (sólo cambia `frames` en el JSON) o animaciones nuevas (añádelas al JSON y al tipo `Activity` en `src/pet/brain.ts`).

## Flujo recomendado con Aseprite (o LibreSprite, gratuito)

1. Crea un lienzo de 48 × 40, modo RGBA, fondo transparente. Usa como guía la paleta de `scripts/generate-sprites.mjs` (chocolate `#6b351f`, fuego `#c97d3c`, violeta audífonos `#7a4fe0`, pañuelo `#7446c2`, contorno `#21110b`).
2. Crea una **etiqueta (tag)** por animación con los nombres de la tabla.
3. Exporta con *File → Export Sprite Sheet*: Layout **By Rows**, *Split Tags* activado, sin bordes ni padding. Obtendrás un PNG con una fila por etiqueta.
4. Guarda el PNG como `public/sprites/canela.png` y actualiza `public/sprites/canela.json`:

```json
{
  "version": 1,
  "image": "canela.png",
  "frameWidth": 48,
  "frameHeight": 40,
  "facing": "right",
  "anchor": { "x": 24, "y": 38 },
  "placeholder": false,
  "animations": {
    "idle": { "row": 0, "frames": 4, "fps": 4, "loop": true }
  }
}
```

5. Ejecuta `npm test`: la prueba `sprites.test.ts` comprueba que estén todas las animaciones y que las filas/fotogramas quepan en la imagen.
6. Para el icono, guarda una versión cuadrada de 1024 × 1024 en `assets/canela-icon.png` y ejecuta `npx tauri icon assets/canela-icon.png` (puedes borrar las carpetas `android/` e `ios/` generadas).

> Ojo: `npm run sprites` **sobrescribe** `canela.png`/`canela.json` con el arte provisional. No lo ejecutes después de integrar tu arte final.
