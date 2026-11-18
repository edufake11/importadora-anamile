# Sprites de Canela: cómo crear e integrar el arte final

El spritesheet incluido (`public/sprites/canela.png`) es **arte provisional** generado por `scripts/generate-sprites.mjs` a partir de formas geométricas. Sirve para que el MVP funcione; para la calidad de la imagen de referencia hay que dibujarlo a mano. El motor de animación no depende del arte: basta con sustituir dos archivos.

## Formato

- **Una fila por animación**, fotogramas de izquierda a derecha, todos del mismo tamaño.
- Tamaño de fotograma actual: **48 × 40 px**; hoja de 12 columnas × 16 filas (puedes usar otro, p. ej. 64 × 48; se indica en el JSON).
- Personaje **mirando a la derecha** (el motor lo espeja para caminar a la izquierda).
- **Fondo transparente** (PNG RGBA). Sin antialiasing en los bordes: píxeles 100 % opacos o 100 % transparentes (el clic-a-través usa la opacidad).
- Las patas deben apoyarse en la misma fila en todos los fotogramas (`anchor.y` en el JSON).
- La cabeza está alrededor de la columna 35 (mirando a la derecha); la burbuja de diálogo apunta ahí. Si cambias mucho la composición, ajusta `headFrameX` en `src/pet/controller.ts`.

## Animaciones requeridas

| Fila | Nombre | Fotogramas | FPS | Bucle | Uso |
|---|---|---|---|---|---|
| 0 | `idle` | 12 | 6 | sí | De pie: la cola se balancea despacio y parpadea |
| 1 | `walk` | 8 | 12 | sí | Caminar (se espeja para ir a la izquierda) |
| 2 | `run` | 6 | 14 | sí | Correr (galope) |
| 3 | `sit` | 8 | 4 | sí | Sentada, barre el suelo con la cola y parpadea |
| 4 | `lie` | 6 | 3 | sí | Acostada, despierta (descansando) |
| 5 | `sleep` | 4 | 2 | sí | Dormida, respiración (las «Zzz» las dibuja la interfaz) |
| 6 | `wake` | 8 | 5 | no | Se despierta, levanta la cabeza, se estira y se sienta |
| 7 | `listen` | 4 | 3 | sí | Escuchando (mientras escribes en el chat o pide permiso) |
| 8 | `talk` | 4 | 8 | sí | Hablando (mientras llega la respuesta) |
| 9 | `think` | 4 | 2 | sí | Pensando (esperando a la IA o usando herramientas) |
| 10 | `curious` | 6 | 6 | no | Inclina la cabeza con curiosidad (al acercar el cursor) |
| 11 | `celebrate` | 6 | 10 | sí | Salta de alegría (trabajo terminado, 3 clics) |
| 12 | `happy` | 6 | 12 | sí | Feliz, mueve la cola rápido (clic, respuesta terminada) |
| 13 | `oops` | 5 | 8 | no | Sorpresa ante un error: saltito hacia atrás |
| 14 | `sad` | 4 | 2.5 | sí | Triste (después de un error) |
| 15 | `dragged` | 4 | 8 | sí | Colgando mientras la arrastras |

Puedes añadir más fotogramas a cualquier animación (sólo cambia `frames` en el JSON) o animaciones nuevas (añádelas al JSON y al tipo `Activity` en `src/pet/brain.ts`).

### Cómo se generan los fotogramas provisionales

`scripts/generate-sprites.mjs` dibuja cada pose con formas y contorno automático:

- **Cola por ángulo** (`tailAt`): el meneo es una oscilación suave entre dos ángulos.
- **Inclinación de cabeza** por desplazamiento de columnas (no por rotación), para que los audífonos y el ojo sigan nítidos.
- **Orejas siempre visibles**: el pañuelo se dibuja primero (en el cuello, detrás de la cabeza) y la oreja encima.

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
