#!/usr/bin/env node
/**
 * Generador de sprites PROVISIONALES de Canela.
 *
 * Dibuja a Canela en pixel art mediante código (formas + contorno automático)
 * y exporta:
 *   public/sprites/canela.png   -> spritesheet (una fila por animación)
 *   public/sprites/canela.json  -> manifiesto que lee el motor de animación
 *   assets/canela-icon.png      -> icono 1024x1024 para `tauri icon`
 *   (opcional) --preview <ruta> -> vista previa ampliada para revisión
 *
 * Es arte de marcador de posición, hecho a propósito para que el MVP funcione
 * sin recursos externos. Para el arte final, un artista puede dibujar los
 * fotogramas en Aseprite/LibreSprite respetando el MISMO formato (ver
 * docs/SPRITES.md) y sobrescribir canela.png / canela.json.
 *
 * Sin dependencias: usa sólo `node:zlib` y `node:fs`.
 */
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// ---------------------------------------------------------------------------
// Paleta (índice 0 = transparente)
// ---------------------------------------------------------------------------
const PALETTE = [
  null,
  '#21110b', // 1 contorno
  '#4a2516', // 2 pelaje oscuro
  '#6b351f', // 3 pelaje chocolate
  '#8c4a2a', // 4 pelaje brillo
  '#a8602c', // 5 fuego oscuro
  '#c97d3c', // 6 fuego (tan)
  '#e3a466', // 7 fuego claro
  '#1c0f0b', // 8 nariz
  '#140c0a', // 9 ojo
  '#6a3a1e', // 10 iris
  '#fff7f0', // 11 brillo blanco
  '#3f2585', // 12 audífono oscuro
  '#7a4fe0', // 13 audífono violeta
  '#b69cff', // 14 audífono brillo
  '#4d2a8a', // 15 pañuelo oscuro
  '#7446c2', // 16 pañuelo
  '#a17be6', // 17 pañuelo brillo
  '#ef7393', // 18 lengua
  '#23183f', // 19 interior almohadilla
].map((hex) => (hex ? [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16), 255] : [0, 0, 0, 0]));

const C = {
  OUT: 1, FUR_D: 2, FUR: 3, FUR_L: 4, TAN_D: 5, TAN: 6, TAN_L: 7, NOSE: 8, EYE: 9, IRIS: 10, WHITE: 11,
  HP_D: 12, HP: 13, HP_L: 14, BAN_D: 15, BAN: 16, BAN_L: 17, TONGUE: 18, CUP_IN: 19,
};

export const FRAME_W = 48;
export const FRAME_H = 40;
/** Fila donde se apoyan las patas (la base del contorno queda en GROUND+1). */
const GROUND = 37;

// ---------------------------------------------------------------------------
// Lienzo indexado + primitivas
// ---------------------------------------------------------------------------
class Layer {
  constructor(w = FRAME_W, h = FRAME_H) {
    this.w = w;
    this.h = h;
    this.px = new Uint8Array(w * h);
  }
  set(x, y, c) {
    x = Math.round(x);
    y = Math.round(y);
    if (x < 0 || y < 0 || x >= this.w || y >= this.h) return;
    this.px[y * this.w + x] = c;
  }
  get(x, y) {
    if (x < 0 || y < 0 || x >= this.w || y >= this.h) return 0;
    return this.px[y * this.w + x];
  }
  ellipse(cx, cy, rx, ry, c) {
    for (let y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) {
      for (let x = Math.floor(cx - rx); x <= Math.ceil(cx + rx); x++) {
        const dx = (x - cx) / rx;
        const dy = (y - cy) / ry;
        if (dx * dx + dy * dy <= 1.0) this.set(x, y, c);
      }
    }
  }
  rect(x0, y0, x1, y1, c) {
    for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) this.set(x, y, c);
  }
  /** Línea gruesa: discos de radio r a lo largo del segmento. */
  line(x0, y0, x1, y1, c, r = 0) {
    const steps = Math.max(1, Math.ceil(Math.hypot(x1 - x0, y1 - y0) * 2));
    for (let i = 0; i <= steps; i++) {
      const t = i / steps;
      const x = x0 + (x1 - x0) * t;
      const y = y0 + (y1 - y0) * t;
      if (r <= 0) this.set(x, y, c);
      else this.ellipse(x, y, r, r, c);
    }
  }
  poly(points, c) {
    // relleno par-impar por escaneo
    const ys = points.map((p) => p[1]);
    for (let y = Math.floor(Math.min(...ys)); y <= Math.ceil(Math.max(...ys)); y++) {
      const xs = [];
      for (let i = 0; i < points.length; i++) {
        const [ax, ay] = points[i];
        const [bx, by] = points[(i + 1) % points.length];
        if ((ay <= y && by > y) || (by <= y && ay > y)) xs.push(ax + ((y - ay) / (by - ay)) * (bx - ax));
      }
      xs.sort((a, b) => a - b);
      for (let k = 0; k + 1 < xs.length; k += 2) {
        for (let x = Math.ceil(xs[k]); x <= Math.floor(xs[k + 1]); x++) this.set(x, y, c);
      }
    }
  }
  /** Añade contorno (4-vecindad) alrededor de lo pintado. */
  outline(c = C.OUT) {
    const add = [];
    for (let y = 0; y < this.h; y++) {
      for (let x = 0; x < this.w; x++) {
        if (this.get(x, y) !== 0) continue;
        if (this.get(x - 1, y) || this.get(x + 1, y) || this.get(x, y - 1) || this.get(x, y + 1)) add.push([x, y]);
      }
    }
    for (const [x, y] of add) this.set(x, y, c);
    return this;
  }
  over(other) {
    for (let i = 0; i < this.px.length; i++) if (other.px[i]) this.px[i] = other.px[i];
    return this;
  }
  shifted(dx, dy) {
    const out = new Layer(this.w, this.h);
    for (let y = 0; y < this.h; y++) for (let x = 0; x < this.w; x++) {
      const c = this.get(x, y);
      if (c) out.set(x + dx, y + dy, c);
    }
    return out;
  }
}

/** Dibuja una pieza en su propia capa, le pone contorno y la compone encima. */
function part(target, draw, withOutline = true) {
  const l = new Layer(target.w, target.h);
  draw(l);
  if (withOutline) l.outline();
  target.over(l);
}

// ---------------------------------------------------------------------------
// Piezas de Canela
// ---------------------------------------------------------------------------
function leg(L, x0, y0, x1, y1, far) {
  part(L, (l) => {
    const midX = x0 + (x1 - x0) * 0.45;
    const midY = y0 + (y1 - y0) * 0.45;
    l.line(x0, y0, midX, midY, far ? C.FUR_D : C.FUR, 1.2);
    l.line(midX, midY, x1, y1 - 1, far ? C.TAN_D : C.TAN, 1.2);
    l.ellipse(x1 + 0.8, y1, 1.9, 1.1, far ? C.TAN_D : C.TAN_L); // pata
  });
}

function tail(L, pts) {
  part(L, (l) => {
    for (let i = 0; i + 1 < pts.length; i++) {
      const r = i === pts.length - 2 ? 0.7 : 1.2;
      l.line(pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1], C.FUR, r);
    }
  });
}

function bodyStand(L, cx, cy, rx = 12, ry = 5) {
  part(L, (l) => {
    l.ellipse(cx, cy, rx, ry, C.FUR);
    l.ellipse(cx - 1, cy - ry * 0.45, rx - 3, ry * 0.35, C.FUR_L); // lomo brillante
    l.ellipse(cx + rx * 0.2, cy + ry * 0.75, rx * 0.55, 1, C.FUR_D); // vientre en sombra
    l.ellipse(cx + rx - 2, cy + 1, 2.4, ry * 0.65, C.TAN); // pecho color fuego
  });
}

/**
 * Cabeza mirando a la derecha. (hx, hy) = centro del cráneo.
 * opts: eyes: open|closed|happy|up|sad|big, mouth: closed|open|tongue,
 *       ear: down|up|fly|droop
 */
function head(T, hx, hy, opts = {}) {
  const { eyes = 'open', mouth = 'closed', ear = 'down', tilt = 0 } = opts;
  const L = new Layer(T.w, T.h);

  // Pañuelo en el cuello, DETRÁS de la cabeza y de la oreja: nunca tapa las orejas.
  part(L, (l) => {
    l.poly([[hx - 7, hy + 4], [hx + 1, hy + 6], [hx - 4, hy + 12]], C.BAN);
    l.line(hx - 6, hy + 5, hx, hy + 6.5, C.BAN_L, 0);
    l.set(hx - 4, hy + 8, C.BAN_D);
    l.set(hx - 4, hy + 7, C.WHITE); // huellita
  });

  // Cráneo + hocico
  part(L, (l) => {
    l.ellipse(hx, hy, 6.8, 6.2, C.FUR);
    l.ellipse(hx - 1, hy - 3, 4, 2, C.FUR_L);
    l.ellipse(hx + 3.5, hy + 3, 3.2, 2.2, C.TAN); // mejilla
    l.ellipse(hx + 6.5, hy + 3, 3.8, 2.3, C.TAN); // hocico
    l.line(hx + 4, hy + 1, hx + 8, hy + 1.6, C.FUR_L, 0); // caña nasal
    l.line(hx + 4, hy + 2, hx + 7, hy + 2, C.TAN_L, 0);
  });

  // Nariz
  L.rect(hx + 9, hy + 1, hx + 10, hy + 2, C.NOSE);
  L.set(hx + 9, hy + 1, C.FUR_L);

  // Boca
  if (mouth === 'closed') {
    L.line(hx + 5, hy + 5, hx + 8, hy + 5, C.OUT);
  } else {
    L.line(hx + 5, hy + 5, hx + 9, hy + 5, C.OUT);
    L.rect(hx + 6, hy + 6, hx + 8, hy + 6, C.OUT);
    if (mouth === 'tongue') {
      L.rect(hx + 6, hy + 6, hx + 7, hy + 7, C.TONGUE);
      L.set(hx + 7, hy + 8, C.TONGUE);
      L.set(hx + 5, hy + 8, 0);
      L.set(hx + 6, hy + 8, C.OUT);
      L.set(hx + 8, hy + 7, C.OUT);
    } else {
      L.set(hx + 7, hy + 6, C.TONGUE);
    }
  }

  // Ceja color fuego
  if (eyes !== 'sad') {
    L.set(hx + 1, hy - 4, C.TAN);
    L.set(hx + 2, hy - 4, C.TAN);
  } else {
    L.set(hx + 1, hy - 3, C.TAN);
    L.set(hx + 2, hy - 4, C.TAN);
  }

  // Ojo grande y brillante
  const ex = hx + 1;
  const ey = hy - 2;
  switch (eyes) {
    case 'closed':
      L.line(ex, ey + 2, ex + 2, ey + 2, C.EYE);
      L.set(ex - 1, ey + 1, C.EYE);
      break;
    case 'happy':
      L.set(ex, ey + 2, C.EYE);
      L.set(ex + 1, ey + 1, C.EYE);
      L.set(ex + 2, ey + 2, C.EYE);
      break;
    case 'up':
      L.rect(ex, ey, ex + 2, ey + 3, C.EYE);
      L.set(ex + 1, ey, C.WHITE);
      L.set(ex + 2, ey, C.WHITE);
      L.set(ex, ey + 3, C.IRIS);
      break;
    case 'sad':
      L.rect(ex, ey + 1, ex + 2, ey + 3, C.EYE);
      L.set(ex, ey + 1, C.WHITE);
      L.set(ex + 2, ey + 3, C.IRIS);
      L.set(ex + 1, ey + 4, C.WHITE); // lagrimita
      break;
    case 'big':
      L.rect(ex - 1, ey - 1, ex + 2, ey + 3, C.EYE);
      L.set(ex - 1, ey - 1, C.WHITE);
      L.set(ex, ey - 1, C.WHITE);
      L.set(ex - 1, ey, C.WHITE);
      L.set(ex + 1, ey + 3, C.IRIS);
      L.set(ex + 2, ey + 2, C.IRIS);
      break;
    default:
      L.rect(ex, ey, ex + 2, ey + 3, C.EYE);
      L.set(ex, ey, C.WHITE);
      L.set(ex + 1, ey + 3, C.IRIS);
      L.set(ex + 2, ey + 3, C.IRIS);
      L.set(ex + 2, ey + 1, C.WHITE);
  }

  // Oreja larga y caída (encima del pañuelo, bajo la almohadilla del audífono)
  part(L, (l) => {
    if (ear === 'up') l.ellipse(hx - 4, hy + 3, 2.8, 4.5, C.FUR_D); // atenta, pero visible
    else if (ear === 'fly') l.ellipse(hx - 7, hy - 0.5, 4, 1.8, C.FUR_D);
    else if (ear === 'droop') l.ellipse(hx - 3, hy + 5, 2.6, 6, C.FUR_D);
    else l.ellipse(hx - 3, hy + 3.5, 3, 5.5, C.FUR_D);
  });

  // Audífonos: diadema + almohadilla
  part(L, (l) => {
    l.line(hx - 3, hy - 4, hx - 2, hy - 7, C.HP_D, 0.8);
    l.line(hx - 2, hy - 8, hx + 3, hy - 8, C.HP_D, 0.8);
    l.line(hx + 3, hy - 8, hx + 5, hy - 6, C.HP_D, 0.8);
    l.line(hx - 1, hy - 8, hx + 2, hy - 8, C.HP, 0);
  });
  part(L, (l) => {
    l.ellipse(hx - 3, hy - 1, 3.4, 3.6, C.HP_D);
    l.ellipse(hx - 3, hy - 1, 2.3, 2.5, C.HP);
    l.set(hx - 4, hy - 3, C.HP_L);
    l.set(hx - 5, hy - 2, C.HP_L);
    // huellita en la almohadilla
    l.set(hx - 3, hy, C.HP_L);
    l.set(hx - 4, hy - 1, C.HP_L);
    l.set(hx - 2, hy - 1, C.HP_L);
  });
  T.over(tilt ? rotated(L, hx, hy + 3, tilt) : L);
}

/**
 * Inclina una capa desplazando cada columna en vertical (cizallamiento).
 * A diferencia de una rotación, no deja huecos ni deforma los detalles:
 * el pixel art se mantiene nítido. deg < 0 = levanta el hocico.
 */
function rotated(src, cx, _cy, deg) {
  const out = new Layer(src.w, src.h);
  const k = Math.tan((deg * Math.PI) / 180);
  for (let y = 0; y < src.h; y++) for (let x = 0; x < src.w; x++) {
    const c = src.get(x, y - Math.round((x - cx) * k));
    if (c) out.set(x, y, c);
  }
  return out;
}

/** Cola definida por ángulo (0° = horizontal hacia atrás, 90° = vertical). */
function tailAt(L, bx, by, ang, len = 9.5, curl = 10) {
  const pts = [[bx, by]];
  let a = ang;
  let x = bx;
  let y = by;
  for (let i = 0; i < 3; i++) {
    const rad = (a * Math.PI) / 180;
    x -= Math.cos(rad) * (len / 3);
    y -= Math.sin(rad) * (len / 3);
    pts.push([x, y]);
    a += curl;
  }
  tail(L, pts);
}

// ---------------------------------------------------------------------------
// Poses
// ---------------------------------------------------------------------------
const G = GROUND;

function poseStand({ legs = [0, 0, 0, 0], lift = [0, 0, 0, 0], tail: tailAng = 60, y = 0, eyes, mouth, ear, tilt = 0, longLegs = 0, headY = 0 } = {}) {
  const L = new Layer();
  const hipY = G - 6 + y;
  const footY = (i) => G + y - lift[i] + longLegs;
  // patas lejanas
  leg(L, 12 + legs[0], hipY, 11 + legs[0], footY(0), true);
  leg(L, 28 + legs[2], hipY, 29 + legs[2], footY(2), true);
  tailAt(L, 10, G - 11 + y, tailAng);
  bodyStand(L, 20, G - 10 + y, 13, 5.5);
  // patas cercanas
  leg(L, 15 + legs[1], hipY, 14 + legs[1], footY(1), false);
  leg(L, 31 + legs[3], hipY, 32 + legs[3], footY(3), false);
  head(L, 35, G - 18 + y + headY, { eyes, mouth, ear, tilt });
  return L;
}

/** Ciclo de caminata de 8 fotogramas (trote diagonal suave). */
function walkFrame(i, n = 8) {
  const p = (i / n) * Math.PI * 2;
  const d = Math.round(1.6 * Math.sin(p));
  const liftA = Math.cos(p) > 0.35 ? 1 : 0;
  const liftB = Math.cos(p) < -0.35 ? 1 : 0;
  return poseStand({
    legs: [d, -d, -d, d],
    lift: [liftA, liftB, liftB, liftA],
    y: Math.abs(Math.sin(p)) < 0.5 ? -1 : 0,
    tail: 55 + 14 * Math.sin(p * 2),
  });
}

const RUN_KEYS = [
  { back: [[12, 7, -2], [14, 9, -1]], front: [[29, 34, -2], [31, 36, -1]], y: -1 },
  { back: [[12, 10, 0], [14, 13, 0]], front: [[29, 32, 0], [31, 33, -1]], y: 0 },
  { back: [[12, 16, -1], [14, 18, 0]], front: [[29, 25, -1], [31, 27, 0]], y: -1 },
  { back: [[12, 12, 0], [14, 15, -1]], front: [[29, 30, 0], [31, 32, 0]], y: 0 },
];

function poseRun(t) {
  // Interpola entre 4 poses clave para obtener un galope más fluido.
  const k = t * RUN_KEYS.length;
  const a = RUN_KEYS[Math.floor(k) % RUN_KEYS.length];
  const b = RUN_KEYS[(Math.floor(k) + 1) % RUN_KEYS.length];
  const f = k - Math.floor(k);
  const lerp = (u, v) => u + (v - u) * f;
  const yy = Math.round(lerp(a.y, b.y));
  const L = new Layer();
  const hip = G - 7 + yy;
  const legOf = (ka, kb, far) => leg(L, ka[0], hip, Math.round(lerp(ka[1], kb[1])), G + Math.round(lerp(ka[2], kb[2])) + yy, far);
  legOf(a.back[0], b.back[0], true);
  legOf(a.front[0], b.front[0], true);
  tailAt(L, 9, G - 12 + yy, 15 + 10 * Math.sin(t * Math.PI * 2), 10, 4);
  bodyStand(L, 20, G - 11 + yy, 13, 5);
  legOf(a.back[1], b.back[1], false);
  legOf(a.front[1], b.front[1], false);
  head(L, 36, G - 19 + yy, { ear: 'fly', mouth: Math.floor(k) % 2 ? 'tongue' : 'open' });
  return L;
}

function poseSit({ tail: tailAng = 8, eyes, mouth, ear, headY = 0, tilt = 0 } = {}) {
  const L = new Layer();
  // pata delantera lejana
  leg(L, 27, G - 8, 27, G, true);
  // cola apoyada en el suelo, barre de un lado a otro
  tailAt(L, 12, G - 2, tailAng, 9, -4);
  // cadera + torso inclinado
  part(L, (l) => {
    l.ellipse(17, G - 5, 7.5, 5, C.FUR);
    l.ellipse(23, G - 9, 7, 6.5, C.FUR);
    l.ellipse(17, G - 8, 4, 1.8, C.FUR_L);
    l.ellipse(27, G - 9, 2.6, 4.5, C.TAN); // pecho
    l.ellipse(13, G - 0.5, 4, 1.2, C.TAN_L); // pata trasera
  });
  leg(L, 30, G - 8, 30, G, false);
  head(L, 33, G - 20 + headY, { eyes, mouth, ear, tilt });
  return L;
}

/** Acostada (despierta o dormida). */
function poseLie({ breathe = 0, eyes = 'open', ear, headUp = true, tail: tailAng = 20, mouth } = {}) {
  const L = new Layer();
  tailAt(L, 9, G - 4, tailAng, 8, -6);
  part(L, (l) => {
    l.ellipse(21, G - 5 - breathe * 0.5, 13, 5 + breathe * 0.5, C.FUR);
    l.ellipse(20, G - 8 - breathe, 9, 1.6, C.FUR_L);
    l.ellipse(37, G, 5, 1.2, C.TAN_L); // patitas delanteras
  });
  if (headUp) head(L, 35, G - 9, { eyes, ear: ear ?? 'down', mouth });
  else head(L, 35, G - 6, { eyes, ear: ear ?? 'droop' });
  return L;
}

/** Estiramiento ("reverencia"): patas delanteras estiradas, cadera arriba. */
function poseBow({ depth = 1, eyes = 'closed', mouth = 'open', tail: tailAng = 75 } = {}) {
  const L = new Layer();
  const d = depth;
  leg(L, 12, G - 9 - d, 11, G, true);
  tailAt(L, 10, G - 13 - d, tailAng);
  part(L, (l) => {
    for (let t = 0; t <= 1.0001; t += 0.1) l.ellipse(13 + 15 * t, G - 12 - d + (6 + 2 * d) * t, 5, 4.6, C.FUR);
    l.line(12, G - 16 - d, 24, G - 12, C.FUR_L, 0.6);
    l.ellipse(27, G - 4 + d * 0.5, 2.4, 2.5, C.TAN); // pecho
  });
  leg(L, 15, G - 9 - d, 14, G, false);
  part(L, (l) => {
    l.line(27, G - 2, 37, G - 1, C.TAN, 1.1); // patas delanteras estiradas
    l.ellipse(38.5, G, 2.2, 1.1, C.TAN_L);
  });
  head(L, 35, G - 9 + Math.round(d), { eyes, mouth, ear: 'down', tilt: -6 });
  return L;
}

// ---------------------------------------------------------------------------
// Animaciones (una fila del spritesheet cada una)
// ---------------------------------------------------------------------------
const seq = (n, f) => Array.from({ length: n }, (_, i) => f(i, n));
const wave = (i, n, a, b) => a + (b - a) * (0.5 - 0.5 * Math.cos((i / n) * Math.PI * 2));

const ANIMATIONS = [
  // Reposo: la cola se balancea despacio y parpadea una vez por ciclo.
  ['idle', 6, true, () => seq(12, (i, n) => poseStand({ tail: wave(i, n, 45, 80), eyes: i === 9 ? 'closed' : 'open' }))],
  ['walk', 12, true, () => seq(8, (i) => walkFrame(i))],
  ['run', 14, true, () => seq(6, (i, n) => poseRun(i / n))],
  ['sit', 4, true, () => seq(8, (i, n) => poseSit({ tail: wave(i, n, 4, 26), eyes: i === 6 ? 'closed' : 'open' }))],
  ['lie', 3, true, () => seq(6, (i, n) => poseLie({ breathe: i % 3 === 1 ? 1 : 0, tail: wave(i, n, 10, 28), eyes: i === 4 ? 'closed' : 'open' }))],
  ['sleep', 2, true, () => [0, 0.5, 1, 0.5].map((b) => poseLie({ breathe: b, eyes: 'closed', headUp: false, tail: 14 }))],
  // Despertarse y estirarse: abre los ojos, levanta la cabeza, se estira y se sienta.
  ['wake', 5, false, () => [
    poseLie({ eyes: 'closed', headUp: false }),
    poseLie({ eyes: 'open', headUp: false }),
    poseLie({ eyes: 'big', headUp: true }),
    poseBow({ depth: 0, eyes: 'closed', mouth: 'closed' }),
    poseBow({ depth: 1, eyes: 'closed', mouth: 'open' }),
    poseBow({ depth: 2, eyes: 'happy', mouth: 'tongue' }),
    poseStand({ tail: 70, eyes: 'happy' }),
    poseSit({ eyes: 'open', tail: 20 }),
  ]],
  ['listen', 3, true, () => [
    poseStand({ ear: 'up', eyes: 'big', tail: 60 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 55, tilt: -5 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 60, tilt: -8 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 55, tilt: -5 }),
  ]],
  ['talk', 8, true, () => [
    poseSit({ mouth: 'open', tail: 10 }),
    poseSit({ mouth: 'closed', tail: 14 }),
    poseSit({ mouth: 'open', tail: 18 }),
    poseSit({ mouth: 'closed', tail: 14, eyes: 'happy' }),
  ]],
  ['think', 2, true, () => [
    poseSit({ eyes: 'up', tail: 8 }),
    poseSit({ eyes: 'up', tail: 14, tilt: -6 }),
    poseSit({ eyes: 'up', tail: 20, tilt: -10 }),
    poseSit({ eyes: 'up', tail: 14, tilt: -6 }),
  ]],
  // Curiosa: inclina la cabeza y se queda mirando.
  ['curious', 6, false, () => [
    poseStand({ ear: 'up', eyes: 'big', tail: 55 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 60, tilt: -6 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 65, tilt: -12 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 70, tilt: -16 }),
    poseStand({ ear: 'up', eyes: 'open', tail: 65, tilt: -16 }),
    poseStand({ ear: 'up', eyes: 'big', tail: 70, tilt: -16 }),
  ]],
  ['celebrate', 10, true, () => [
    poseStand({ y: 2, longLegs: -2, eyes: 'happy', mouth: 'tongue', tail: 70 }),
    poseStand({ y: -2, longLegs: 1, eyes: 'happy', mouth: 'tongue', ear: 'up', tail: 85 }),
    poseStand({ y: -5, longLegs: 2, eyes: 'happy', mouth: 'tongue', ear: 'up', tail: 40 }),
    poseStand({ y: -6, longLegs: 2, eyes: 'happy', mouth: 'open', ear: 'fly', tail: 85 }),
    poseStand({ y: -3, longLegs: 1, eyes: 'happy', mouth: 'tongue', ear: 'up', tail: 40 }),
    poseStand({ y: 0, eyes: 'happy', mouth: 'tongue', tail: 80 }),
  ]],
  // Feliz: mueve la cola rápido de lado a lado.
  ['happy', 12, true, () => seq(6, (i, n) => poseStand({ eyes: 'happy', mouth: 'tongue', tail: wave(i, n, 30, 90) }))],
  // Sorpresa ante un error: da un saltito hacia atrás con los ojos muy abiertos.
  ['oops', 8, false, () => [
    poseStand({ eyes: 'big', mouth: 'open', ear: 'up', tail: 35 }),
    poseStand({ y: -2, eyes: 'big', mouth: 'open', ear: 'fly', tail: 30, legs: [-1, -1, -1, -1] }),
    poseStand({ y: -1, eyes: 'big', mouth: 'open', ear: 'up', tail: 30, legs: [-1, -1, -1, -1] }),
    poseStand({ eyes: 'big', mouth: 'closed', ear: 'up', tail: 30 }),
    poseStand({ eyes: 'sad', ear: 'droop', tail: 25, headY: 1 }),
  ]],
  ['sad', 2.5, true, () => [
    poseSit({ eyes: 'sad', ear: 'droop', headY: 1, tail: 2 }),
    poseSit({ eyes: 'sad', ear: 'droop', headY: 2, tail: 4 }),
    poseSit({ eyes: 'sad', ear: 'droop', headY: 2, tail: 2 }),
    poseSit({ eyes: 'closed', ear: 'droop', headY: 1, tail: 4 }),
  ]],
  ['dragged', 8, true, () => [
    poseStand({ y: -3, longLegs: 3, legs: [-1, 1, 1, -1], mouth: 'open', eyes: 'big', ear: 'fly', tail: 30 }),
    poseStand({ y: -3, longLegs: 3, legs: [0, 0, 0, 0], mouth: 'open', eyes: 'big', ear: 'fly', tail: 40 }),
    poseStand({ y: -3, longLegs: 3, legs: [1, -1, -1, 1], mouth: 'open', eyes: 'big', ear: 'fly', tail: 30 }),
    poseStand({ y: -3, longLegs: 3, legs: [0, 0, 0, 0], mouth: 'open', eyes: 'big', ear: 'fly', tail: 40 }),
  ]],
];

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------
const CRC_TABLE = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
export function encodePng(width, height, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bits
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, rgba.byteOffset + y * width * 4, width * 4).copy(raw, y * (width * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

// ---------------------------------------------------------------------------
// Construcción
// ---------------------------------------------------------------------------
function build() {
  const cols = Math.max(...ANIMATIONS.map(([, , , f]) => f().length));
  const sheetW = cols * FRAME_W;
  const sheetH = ANIMATIONS.length * FRAME_H;
  const rgba = new Uint8Array(sheetW * sheetH * 4);
  const manifest = {
    $comment: 'Spritesheet de Canela. Una fila por animación; los fotogramas van de izquierda a derecha. Ver docs/SPRITES.md',
    version: 1,
    image: 'canela.png',
    frameWidth: FRAME_W,
    frameHeight: FRAME_H,
    facing: 'right',
    anchor: { x: FRAME_W / 2, y: GROUND + 1 },
    placeholder: true,
    animations: {},
  };
  const framesByAnim = {};
  ANIMATIONS.forEach(([name, fps, loop, make], row) => {
    const frames = make();
    framesByAnim[name] = frames;
    manifest.animations[name] = { row, frames: frames.length, fps, loop };
    frames.forEach((layer, col) => {
      for (let y = 0; y < FRAME_H; y++) for (let x = 0; x < FRAME_W; x++) {
        const c = layer.get(x, y);
        if (!c) continue;
        const o = ((row * FRAME_H + y) * sheetW + col * FRAME_W + x) * 4;
        rgba.set(PALETTE[c], o);
      }
    });
  });
  return { rgba, sheetW, sheetH, manifest, framesByAnim };
}

function scaleUp(rgba, w, h, s, bg) {
  const out = new Uint8Array(w * s * h * s * 4);
  for (let y = 0; y < h * s; y++) for (let x = 0; x < w * s; x++) {
    const si = (Math.floor(y / s) * w + Math.floor(x / s)) * 4;
    const o = (y * w * s + x) * 4;
    if (rgba[si + 3]) out.set(rgba.subarray(si, si + 4), o);
    else if (bg) out.set(bg, o);
  }
  return out;
}

function buildIcon(frame) {
  // Recorta al contenido y centra sobre un fondo violeta oscuro redondeado.
  let minX = FRAME_W, minY = FRAME_H, maxX = 0, maxY = 0;
  for (let y = 0; y < FRAME_H; y++) for (let x = 0; x < FRAME_W; x++) if (frame.get(x, y)) {
    minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
  }
  const S = 1024;
  const out = new Uint8Array(S * S * 4);
  const radius = 200;
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    const cx = Math.min(Math.max(x, radius), S - 1 - radius);
    const cy = Math.min(Math.max(y, radius), S - 1 - radius);
    if (Math.hypot(x - cx, y - cy) > radius) continue;
    const t = y / S;
    const o = (y * S + x) * 4;
    out.set([Math.round(22 + 30 * t), Math.round(12 + 8 * t), Math.round(44 + 50 * t), 255], o);
  }
  const cw = maxX - minX + 1;
  const ch = maxY - minY + 1;
  const scale = Math.floor(Math.min(820 / cw, 820 / ch));
  const ox = Math.floor((S - cw * scale) / 2);
  const oy = Math.floor((S - ch * scale) / 2) + 20;
  for (let y = 0; y < ch * scale; y++) for (let x = 0; x < cw * scale; x++) {
    const c = frame.get(minX + Math.floor(x / scale), minY + Math.floor(y / scale));
    if (c) out.set(PALETTE[c], ((oy + y) * S + ox + x) * 4);
  }
  return encodePng(S, S, out);
}

function main() {
  const { rgba, sheetW, sheetH, manifest, framesByAnim } = build();
  const spriteDir = resolve(ROOT, 'public/sprites');
  mkdirSync(spriteDir, { recursive: true });
  writeFileSync(resolve(spriteDir, 'canela.png'), encodePng(sheetW, sheetH, rgba));
  writeFileSync(resolve(spriteDir, 'canela.json'), JSON.stringify(manifest, null, 2) + '\n');

  // Cara para el panel de chat (fotograma "sit" recortado y ampliado x4).
  const sit = framesByAnim.sit[0];
  const crop = new Layer(30, 28);
  for (let y = 0; y < 28; y++) for (let x = 0; x < 30; x++) crop.set(x, y, sit.get(18 + x, 6 + y));
  const face = new Uint8Array(30 * 28 * 4);
  for (let i = 0; i < crop.px.length; i++) face.set(PALETTE[crop.px[i]], i * 4);
  writeFileSync(resolve(spriteDir, 'canela-face.png'), encodePng(120, 112, scaleUp(face, 30, 28, 4)));

  mkdirSync(resolve(ROOT, 'assets'), { recursive: true });
  writeFileSync(resolve(ROOT, 'assets/canela-icon.png'), buildIcon(framesByAnim.sit[0]));

  const pi = process.argv.indexOf('--preview');
  if (pi > 0 && process.argv[pi + 1]) {
    const s = 6;
    writeFileSync(process.argv[pi + 1], encodePng(sheetW * s, sheetH * s, scaleUp(rgba, sheetW, sheetH, s, [18, 12, 34, 255])));
  }
  console.log(`Spritesheet ${sheetW}x${sheetH} con ${Object.keys(manifest.animations).length} animaciones -> public/sprites/canela.png`);
}

main();
