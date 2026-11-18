// Motor de animación por spritesheet.
// Formato (ver docs/SPRITES.md): una fila por animación, fotogramas de
// izquierda a derecha, todos del mismo tamaño, personaje mirando a la derecha.

export interface AnimationDef {
  row: number;
  frames: number;
  fps: number;
  loop: boolean;
}

export interface SpriteManifest {
  version: number;
  image: string;
  frameWidth: number;
  frameHeight: number;
  facing: 'right' | 'left';
  anchor: { x: number; y: number };
  placeholder?: boolean;
  animations: Record<string, AnimationDef>;
}

export const REQUIRED_ANIMATIONS = [
  'idle', 'walk', 'run', 'sit', 'lie', 'sleep', 'wake', 'listen', 'talk', 'think', 'curious', 'celebrate', 'happy', 'oops',
  'sad', 'dragged',
] as const;

/** Valida el manifiesto; devuelve la lista de problemas (vacía si es válido). */
export function validateManifest(m: SpriteManifest, imageWidth?: number, imageHeight?: number): string[] {
  const problems: string[] = [];
  if (!(m.frameWidth > 0 && m.frameHeight > 0)) problems.push('frameWidth/frameHeight deben ser > 0');
  for (const name of REQUIRED_ANIMATIONS) {
    if (!m.animations[name]) problems.push(`falta la animación "${name}"`);
  }
  for (const [name, a] of Object.entries(m.animations)) {
    if (a.frames < 1) problems.push(`${name}: necesita al menos 1 fotograma`);
    if (a.fps <= 0) problems.push(`${name}: fps debe ser > 0`);
    if (imageWidth !== undefined && a.frames * m.frameWidth > imageWidth) problems.push(`${name}: los fotogramas exceden el ancho de la imagen`);
    if (imageHeight !== undefined && (a.row + 1) * m.frameHeight > imageHeight) problems.push(`${name}: la fila ${a.row} excede el alto de la imagen`);
  }
  return problems;
}

/** Índice del fotograma para una animación que empezó en `startedAt`. */
export function frameIndex(a: AnimationDef, startedAt: number, now: number): number {
  const elapsed = Math.max(0, now - startedAt);
  const i = Math.floor((elapsed / 1000) * a.fps);
  return a.loop ? i % a.frames : Math.min(i, a.frames - 1);
}

export class SpriteSheet {
  readonly alpha: Uint8Array;

  private constructor(
    readonly manifest: SpriteManifest,
    readonly image: HTMLImageElement,
  ) {
    // Máscara de opacidad para el hit-test pixel a pixel (clic a través de lo transparente).
    const c = document.createElement('canvas');
    c.width = image.naturalWidth;
    c.height = image.naturalHeight;
    const ctx = c.getContext('2d', { willReadFrequently: true })!;
    ctx.drawImage(image, 0, 0);
    const data = ctx.getImageData(0, 0, c.width, c.height).data;
    this.alpha = new Uint8Array(c.width * c.height);
    for (let i = 0; i < this.alpha.length; i++) this.alpha[i] = data[i * 4 + 3];
  }

  static async load(base = '/sprites/canela.json'): Promise<SpriteSheet> {
    const manifest = (await (await fetch(base)).json()) as SpriteManifest;
    const url = new URL(manifest.image, new URL(base, window.location.href)).toString();
    const image = new Image();
    image.src = url;
    await image.decode();
    const problems = validateManifest(manifest, image.naturalWidth, image.naturalHeight);
    if (problems.length) throw new Error('Spritesheet inválido: ' + problems.join('; '));
    return new SpriteSheet(manifest, image);
  }

  anim(name: string): AnimationDef {
    return this.manifest.animations[name] ?? this.manifest.animations.idle;
  }

  /** Dibuja un fotograma escalado y opcionalmente espejado. */
  draw(ctx: CanvasRenderingContext2D, name: string, frame: number, scale: number, flip: boolean) {
    const { frameWidth: w, frameHeight: h } = this.manifest;
    const a = this.anim(name);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
    ctx.save();
    if (flip) {
      ctx.translate(w * scale, 0);
      ctx.scale(-1, 1);
    }
    ctx.drawImage(this.image, frame * w, a.row * h, w, h, 0, 0, w * scale, h * scale);
    ctx.restore();
  }

  /** ¿Es opaco el píxel (fx, fy) del fotograma? */
  isOpaque(name: string, frame: number, fx: number, fy: number, flip: boolean): boolean {
    const { frameWidth: w, frameHeight: h } = this.manifest;
    if (fx < 0 || fy < 0 || fx >= w || fy >= h) return false;
    const x = flip ? w - 1 - fx : fx;
    const a = this.anim(name);
    const sx = frame * w + x;
    const sy = a.row * h + fy;
    return this.alpha[sy * this.image.naturalWidth + sx] > 32;
  }
}
