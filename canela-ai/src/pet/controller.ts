// Controlador imperativo de la ventana de Canela: une la máquina de estados,
// el motor de sprites y las APIs de ventana de Tauri.
import { CheckMenuItem, Menu, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu';
import { listen } from '@tauri-apps/api/event';
import { cursorPosition, currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window';
import { api, type Settings } from '../lib/ipc';
import { type Activity, type BrainContext, type BrainEvent, type BrainState, type ExternalMood, initialState, speedFor, step } from './brain';
import { frameIndex, type SpriteSheet } from './sprites';

/** Espacio (px lógicos) reservado encima de Canela para las burbujas. */
export const BUBBLE_AREA = 120;
export const MIN_WIDTH = 240;

export interface PetUi {
  onActivity(a: Activity): void;
  onLayout(l: PetLayout): void;
  say(text: string, ms?: number): void;
}

export interface PetLayout {
  width: number;
  height: number;
  canvasW: number;
  canvasH: number;
  scale: number;
  /** X (px lógicos) de la cabeza, para apuntar la burbuja. */
  headX: number;
  /** Y (px lógicos) de la parte superior de la cabeza. */
  headY: number;
  flip: boolean;
}

const CLICK_PHRASES = [
  '¡Guau! 🐾',
  '¿Me rascas la pancita?',
  'Doble clic para hablar conmigo 💜',
  '¿Programamos algo hoy?',
  '¡Aquí estoy si me necesitas!',
  'Clic derecho para ver mis opciones.',
];

interface Area {
  x: number;
  y: number;
  w: number;
  h: number;
}

export class PetController {
  private win = getCurrentWindow();
  private brain: BrainState = initialState(performance.now());
  private anim: Activity = 'idle';
  private animStart = performance.now();
  private flip = false;
  private pos = { x: 0, y: 0 }; // físico
  private sf = 1;
  private area: Area = { x: 0, y: 0, w: 1920, h: 1040 };
  private layout!: PetLayout;
  private ignoring: boolean | null = null;
  private dragging = false;
  private dragTimer: number | undefined;
  private downAt: { x: number; y: number } | null = null;
  private clickTimes: number[] = [];
  private moveBusy = false;
  private hitBusy = false;
  private lastMove = performance.now();
  private lastDrawKey = '';
  private timers: number[] = [];
  private unlisten: Array<() => void> = [];
  private renderTimer: number | undefined;
  private hitTimer: number | undefined;
  private moveTimer: number | undefined;
  /** Ventana oculta (atajo, bandeja o pantalla completa): no se trabaja. */
  private paused = false;
  private cursorInside = false;
  private lastNoticed = 0;
  private disposed = false;

  constructor(
    private canvas: HTMLCanvasElement,
    private sheet: SpriteSheet,
    private settings: Settings,
    private ui: PetUi,
  ) {}

  // --------------------------------------------------------------- ciclo

  async start() {
    this.sf = await this.win.scaleFactor();
    await this.refreshArea();
    await this.applyScale(this.settings.petScale);
    // Arranca en la esquina inferior derecha, sobre la barra de tareas.
    const physW = this.layout.width * this.sf;
    this.pos = { x: Math.round(this.area.x + this.area.w - physW - 60 * this.sf), y: this.floorY() };
    await this.win.setPosition(new PhysicalPosition(this.pos.x, this.pos.y));

    this.unlisten.push(
      await this.win.onMoved(({ payload }) => {
        this.pos = { x: payload.x, y: payload.y };
        if (this.dragging) this.armDragEnd();
      }),
      await this.win.onScaleChanged(({ payload }) => {
        this.sf = payload.scaleFactor;
        void this.refreshArea();
      }),
    );

    this.unlisten.push(
      await listen<boolean>('pet-visibility', ({ payload }) => {
        this.paused = !payload;
        if (!this.paused) {
          this.lastDrawKey = '';
          this.scheduleRender(0);
          this.scheduleHit(0);
          this.scheduleMove(0);
        }
      }),
    );

    this.timers.push(
      window.setInterval(() => !this.paused && this.dispatch({ type: 'tick', now: performance.now() }), 150),
      window.setInterval(() => !this.paused && void this.refreshArea(), 5000),
    );
    this.scheduleRender(0);
    this.scheduleHit(0);
    this.scheduleMove(0);
    this.ui.say('¡Hola! Soy Canela 🐾 Doble clic para hablar conmigo.', 5000);
  }

  dispose() {
    this.disposed = true;
    window.clearTimeout(this.renderTimer);
    window.clearTimeout(this.hitTimer);
    window.clearTimeout(this.moveTimer);
    this.timers.forEach((t) => clearInterval(t));
    window.clearTimeout(this.dragTimer);
    this.unlisten.forEach((u) => u());
  }

  // ------------------------------------------------------------ cerebro

  private ctx(): BrainContext {
    return {
      rng: Math.random,
      wander: this.settings.wander && !this.settings.reduceMotion,
      pickTarget: (run) => this.pickTarget(run),
    };
  }

  dispatch(e: BrainEvent) {
    const prev = this.brain.activity;
    this.brain = step(this.brain, e, this.ctx());
    if (this.brain.activity !== prev || e.type === 'click') {
      if (this.brain.activity !== prev) {
        this.anim = this.brain.activity;
        this.animStart = performance.now();
        this.ui.onActivity(this.anim);
        this.scheduleRender(0);
        if (speedFor(this.anim) > 0) this.scheduleMove(0);
      }
    }
  }

  mood(mood: ExternalMood, bubble?: string) {
    this.dispatch({ type: 'mood', mood, now: performance.now() });
    if (bubble) this.ui.say(bubble, mood === 'talk' ? 8000 : 6000);
  }

  async updateSettings(s: Settings) {
    const scaleChanged = s.petScale !== this.settings.petScale;
    this.settings = s;
    this.scheduleRender(0);
    if (scaleChanged) {
      await this.applyScale(s.petScale);
      this.pos.y = Math.min(this.pos.y, this.floorY());
      await this.win.setPosition(new PhysicalPosition(this.pos.x, this.pos.y));
    }
    if ((!s.wander || s.reduceMotion) && (this.brain.activity === 'walk' || this.brain.activity === 'run')) {
      this.dispatch({ type: 'command', activity: 'idle', now: performance.now() });
    }
  }

  // ---------------------------------------------------------- geometría

  private async refreshArea() {
    const m = await currentMonitor();
    if (!m) return;
    const wa = m.workArea ?? { position: m.position, size: m.size };
    this.area = { x: wa.position.x, y: wa.position.y, w: wa.size.width, h: wa.size.height };
  }

  private async applyScale(scale: number) {
    const { frameWidth: fw, frameHeight: fh } = this.sheet.manifest;
    const canvasW = fw * scale;
    const canvasH = fh * scale;
    const width = Math.max(MIN_WIDTH, canvasW + 24);
    const height = canvasH + BUBBLE_AREA;
    const dpr = window.devicePixelRatio || 1;
    this.canvas.width = Math.round(canvasW * dpr);
    this.canvas.height = Math.round(canvasH * dpr);
    this.canvas.style.width = `${canvasW}px`;
    this.canvas.style.height = `${canvasH}px`;
    this.layout = { width, height, canvasW, canvasH, scale, headX: 0, headY: 0, flip: this.flip };
    this.updateHead();
    this.lastDrawKey = '';
    await this.win.setSize(new LogicalSize(width, height));
  }

  private updateHead() {
    const l = this.layout;
    const left = (l.width - l.canvasW) / 2;
    const headFrameX = this.flip ? 13 : 35; // columna de la cabeza en el sprite de 48 px
    l.flip = this.flip;
    l.headX = left + headFrameX * l.scale;
    l.headY = BUBBLE_AREA + 8 * l.scale;
    this.ui.onLayout({ ...l });
  }

  /** Y física en la que las patas tocan el borde inferior del área de trabajo. */
  private floorY() {
    const { frameHeight, anchor } = this.sheet.manifest;
    const physH = this.layout.height * this.sf;
    const below = (frameHeight - anchor.y) * this.layout.scale * this.sf;
    return Math.round(this.area.y + this.area.h - physH + below);
  }

  private pickTarget(running: boolean) {
    const physW = this.layout.width * this.sf;
    const minX = this.area.x;
    const maxX = this.area.x + this.area.w - physW;
    let x = minX + Math.random() * Math.max(0, maxX - minX);
    if (running) {
      // Las carreras van lejos: al lado opuesto de la pantalla.
      const mid = this.area.x + this.area.w / 2;
      x = this.pos.x < mid ? Math.max(x, mid) : Math.min(x, mid - physW);
    }
    const floor = this.floorY();
    const y = Math.random() < 0.75 ? floor : this.area.y + Math.random() * Math.max(0, floor - this.area.y);
    return { x: Math.round(x), y: Math.round(y) };
  }

  /** El movimiento sólo usa un temporizador rápido mientras Canela camina. */
  private scheduleMove(ms: number) {
    window.clearTimeout(this.moveTimer);
    if (this.disposed) return;
    this.moveTimer = window.setTimeout(async () => {
      await this.moveStep();
      const moving = !this.paused && speedFor(this.brain.activity) > 0 && !!this.brain.target;
      if (moving) this.scheduleMove(33);
    }, ms);
  }

  private async moveStep() {
    const now = performance.now();
    const dt = Math.min(0.1, (now - this.lastMove) / 1000);
    this.lastMove = now;
    const a = this.brain.activity;
    const t = this.brain.target;
    if (!t || this.dragging || this.moveBusy || speedFor(a) === 0) return;
    const dx = t.x - this.pos.x;
    const dy = t.y - this.pos.y;
    const dist = Math.hypot(dx, dy);
    const stepLen = speedFor(a) * this.sf * dt;
    const flip = dx < 0;
    if (flip !== this.flip && Math.abs(dx) > 2) {
      this.flip = flip;
      this.updateHead();
    }
    if (dist <= stepLen || dist < 1) {
      this.pos = { x: t.x, y: t.y };
      this.dispatch({ type: 'arrived', now });
    } else {
      this.pos = { x: this.pos.x + (dx / dist) * stepLen, y: this.pos.y + (dy / dist) * stepLen };
    }
    this.moveBusy = true;
    try {
      await this.win.setPosition(new PhysicalPosition(Math.round(this.pos.x), Math.round(this.pos.y)));
    } finally {
      this.moveBusy = false;
    }
  }

  // ------------------------------------------------------------ dibujo

  /** Con "animaciones reducidas" Canela se anima a la mitad de velocidad. */
  private animDef() {
    const a = this.sheet.anim(this.anim);
    return this.settings.reduceMotion ? { ...a, fps: a.fps / 2 } : a;
  }

  private currentFrame(now = performance.now()) {
    return frameIndex(this.animDef(), this.animStart, now);
  }

  /**
   * Dibuja sólo cuando cambia el fotograma y programa el siguiente cambio,
   * en lugar de redibujar 60 veces por segundo.
   */
  private scheduleRender(ms: number) {
    window.clearTimeout(this.renderTimer);
    if (this.disposed || this.paused) return;
    this.renderTimer = window.setTimeout(() => {
      this.render();
      const a = this.animDef();
      const now = performance.now();
      const frameMs = 1000 / a.fps;
      const elapsed = now - this.animStart;
      const finished = !a.loop && elapsed >= a.frames * frameMs;
      if (!finished) this.scheduleRender(Math.max(8, frameMs - (elapsed % frameMs)));
    }, ms);
  }

  private render() {
    const frame = this.currentFrame();
    const key = `${this.anim}:${frame}:${this.flip}:${this.layout.scale}`;
    if (key === this.lastDrawKey) return;
    this.lastDrawKey = key;
    const ctx = this.canvas.getContext('2d')!;
    const dpr = this.canvas.width / this.layout.canvasW;
    this.sheet.draw(ctx, this.anim, frame, this.layout.scale * dpr, this.flip);
  }

  // ---------------------------------------- clic a través de lo transparente

  /**
   * Las ventanas transparentes capturan el ratón aunque el píxel sea invisible.
   * Consultamos la posición del cursor y sólo aceptamos clics cuando está sobre
   * un píxel opaco de Canela o sobre un elemento marcado con `data-hit`.
   */
  /** Consulta el cursor a menudo sólo cuando está cerca de Canela. */
  private scheduleHit(ms: number) {
    window.clearTimeout(this.hitTimer);
    if (this.disposed || this.paused) return;
    this.hitTimer = window.setTimeout(async () => {
      const dist = await this.hitTest();
      this.scheduleHit(dist < 160 ? 50 : dist < 400 ? 150 : 300);
    }, ms);
  }

  /** Devuelve la distancia (px lógicos) del cursor a la ventana. */
  private async hitTest(): Promise<number> {
    if (this.hitBusy || this.dragging) return 0;
    this.hitBusy = true;
    try {
      const cur = await cursorPosition();
      const lx = (cur.x - this.pos.x) / this.sf;
      const ly = (cur.y - this.pos.y) / this.sf;
      const dx = lx < 0 ? -lx : lx > this.layout.width ? lx - this.layout.width : 0;
      const dy = ly < 0 ? -ly : ly > this.layout.height ? ly - this.layout.height : 0;
      const dist = Math.hypot(dx, dy);
      // Si el cursor se acerca a Canela mientras descansa, se pone curiosa.
      const inside = dist === 0 && ly > BUBBLE_AREA;
      const now = performance.now();
      if (inside && !this.cursorInside && now - this.lastNoticed > 30_000) {
        this.lastNoticed = now;
        this.dispatch({ type: 'noticed', now });
      }
      this.cursorInside = inside;
      const hit = this.isInteractiveAt(lx, ly);
      if (this.ignoring !== !hit) {
        this.ignoring = !hit;
        await this.win.setIgnoreCursorEvents(!hit);
      }
      return dist;
    } catch {
      /* la ventana puede estar oculta; reintentamos en el próximo ciclo */
      return 1000;
    } finally {
      this.hitBusy = false;
    }
  }

  isInteractiveAt(lx: number, ly: number): boolean {
    if (lx < 0 || ly < 0 || lx >= this.layout.width || ly >= this.layout.height) return false;
    const el = document.elementFromPoint(lx, ly);
    if (!el) return false;
    if (el.closest('[data-hit]')) return true;
    if (el !== this.canvas) return false;
    const r = this.canvas.getBoundingClientRect();
    const s = this.layout.scale;
    const fx = Math.floor((lx - r.left) / s);
    const fy = Math.floor((ly - r.top) / s);
    const frame = this.currentFrame();
    // 1 px de tolerancia para que sea fácil atraparla.
    for (const [ox, oy] of [[0, 0], [1, 0], [-1, 0], [0, 1], [0, -1]]) {
      if (this.sheet.isOpaque(this.anim, frame, fx + ox, fy + oy, this.flip)) return true;
    }
    return false;
  }

  // ------------------------------------------------------ ratón / arrastre

  onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    this.downAt = { x: e.screenX, y: e.screenY };
  }

  onPointerMove(e: PointerEvent) {
    if (!this.downAt || this.dragging || (e.buttons & 1) === 0) return;
    if (Math.hypot(e.screenX - this.downAt.x, e.screenY - this.downAt.y) > 4) {
      this.downAt = null;
      void this.startDrag();
    }
  }

  onPointerUp(e: PointerEvent) {
    if (e.button !== 0 || !this.downAt) return;
    this.downAt = null;
    if (!this.dragging) this.click();
  }

  private async startDrag() {
    this.dragging = true;
    this.dispatch({ type: 'dragStart', now: performance.now() });
    this.armDragEnd(700);
    try {
      await this.win.startDragging();
    } catch {
      /* ignorado */
    }
    this.armDragEnd();
  }

  /** El arrastre nativo no avisa al terminar: lo damos por terminado tras una pausa sin movimiento. */
  private armDragEnd(ms = 350) {
    window.clearTimeout(this.dragTimer);
    this.dragTimer = window.setTimeout(() => {
      this.dragging = false;
      this.scheduleHit(0);
      this.dispatch({ type: 'dragEnd', now: performance.now() });
      void this.refreshArea();
    }, ms);
  }

  private click() {
    const now = performance.now();
    this.clickTimes = [...this.clickTimes.filter((t) => now - t < 1200), now];
    const before = this.brain.activity;
    this.dispatch({ type: 'click', now, count: this.clickTimes.length });
    const after = this.brain.activity;
    if (before === 'sleep') this.ui.say('¡Ya desperté! 🐾', 2500);
    else if (after === 'celebrate') this.ui.say('¡Yupiii! 🎉', 2000);
    else if (after === 'happy') this.ui.say(CLICK_PHRASES[Math.floor(Math.random() * CLICK_PHRASES.length)], 3500);
  }

  // ----------------------------------------------------------- menú nativo

  async openContextMenu() {
    const now = () => performance.now();
    const sizes = await Promise.all(
      [2, 3, 4, 5, 6].map((n) =>
        CheckMenuItem.new({
          text: `${n}x`,
          checked: this.settings.petScale === n,
          action: () => void api.saveSettings({ ...this.settings, petScale: n }),
        }),
      ),
    );
    const sleeping = this.brain.activity === 'sleep';
    const menu = await Menu.new({
      items: [
        { text: 'Abrir asistente', action: () => void api.showPanel() },
        await PredefinedMenuItem.new({ item: 'Separator' }),
        sleeping
          ? { text: 'Despertar', action: () => this.dispatch({ type: 'click', now: now(), count: 1 }) }
          : { text: 'Dormir', action: () => this.dispatch({ type: 'command', activity: 'sleep', now: now() }) },
        { text: 'Sentarse', action: () => this.dispatch({ type: 'command', activity: 'sit', now: now() }) },
        { text: 'Dar un paseo', enabled: this.settings.wander, action: () => this.dispatch({ type: 'command', activity: 'walk', now: now() }) },
        await CheckMenuItem.new({
          text: 'Pasear libremente',
          checked: this.settings.wander,
          action: () => void api.saveSettings({ ...this.settings, wander: !this.settings.wander }),
        }),
        await Submenu.new({ text: 'Tamaño', items: sizes }),
        await PredefinedMenuItem.new({ item: 'Separator' }),
        { text: 'Ocultar   (Ctrl+Alt+C)', action: () => void api.togglePet() },
        { text: 'Salir de Canela AI', action: () => void api.quit() },
      ],
    });
    await menu.popup();
  }
}
