// Máquina de estados de Canela. Es una función pura (sin DOM ni Tauri) para
// poder probarla: recibe el estado actual + un evento y devuelve el nuevo estado.

export type Activity =
  | 'idle'
  | 'walk'
  | 'run'
  | 'sit'
  | 'sleep'
  | 'wake'
  | 'listen'
  | 'talk'
  | 'think'
  | 'celebrate'
  | 'happy'
  | 'sad'
  | 'lie'
  | 'curious'
  | 'oops'
  | 'dragged';

export type ExternalMood = 'idle' | 'listen' | 'think' | 'talk' | 'happy' | 'sad' | 'celebrate';

export interface Point {
  x: number;
  y: number;
}

export interface BrainState {
  activity: Activity;
  /** Momento (ms) en que termina la actividad; Infinity = hasta nuevo evento. */
  until: number;
  target: Point | null;
  /** Último contacto del usuario (clic, arrastre, chat). */
  lastInteraction: number;
}

export type BrainEvent =
  | { type: 'tick'; now: number }
  | { type: 'arrived'; now: number }
  | { type: 'click'; now: number; count: number }
  | { type: 'dragStart'; now: number }
  | { type: 'dragEnd'; now: number }
  | { type: 'mood'; mood: ExternalMood; now: number }
  /** El cursor se acercó a Canela. */
  | { type: 'noticed'; now: number }
  | { type: 'command'; activity: 'sleep' | 'sit' | 'idle' | 'walk' | 'lie'; now: number };

export interface BrainContext {
  rng: () => number;
  wander: boolean;
  /** Elige un destino en el escritorio. */
  pickTarget: (running: boolean) => Point;
}

/** Duraciones en ms [mín, máx]. */
export const DURATIONS: Record<string, [number, number]> = {
  idle: [3000, 6000],
  sit: [6000, 12000],
  sleep: [25000, 60000],
  wake: [1600, 1600], // 8 fotogramas a 5 fps: despertarse y estirarse
  lie: [10000, 20000],
  curious: [2500, 3500],
  oops: [700, 700],
  happy: [1800, 2400],
  celebrate: [2000, 2000],
  sad: [3500, 4500],
  walkTimeout: [20000, 20000],
};

/** Tras este tiempo sin interacción, dormir es mucho más probable. */
export const SLEEPY_AFTER_MS = 2 * 60_000;

/** Actividades controladas desde fuera (chat) que no expiran solas. */
const HELD: Activity[] = ['listen', 'think', 'talk', 'dragged'];

export function initialState(now: number): BrainState {
  return { activity: 'idle', until: now + 4000, target: null, lastInteraction: now };
}

function lasting(activity: Activity, now: number, ctx: BrainContext, lastInteraction: number, target: Point | null = null): BrainState {
  const [a, b] = DURATIONS[activity === 'walk' || activity === 'run' ? 'walkTimeout' : activity] ?? [Infinity, Infinity];
  const until = HELD.includes(activity) ? Infinity : a + (b - a) * ctx.rng() + now;
  return { activity, until, target, lastInteraction };
}

/**
 * Decide la próxima actividad autónoma cuando la actual termina.
 * Canela descansa de forma progresiva: de pie → sentada/acostada → dormida,
 * y con el tiempo sin interacción el sueño se vuelve más probable.
 */
export function chooseNext(s: BrainState, now: number, ctx: BrainContext): BrainState {
  const sleepy = now - s.lastInteraction > SLEEPY_AFTER_MS;
  const resting = s.activity === 'lie';
  const options: Array<[Activity, number]> = [
    ['idle', 20],
    ['sit', 22],
    ['walk', ctx.wander ? 30 : 0],
    ['run', ctx.wander ? 6 : 0],
    ['lie', sleepy ? 30 : 12],
    ['curious', 5],
    // Sólo se duerme desde acostada (o si lleva mucho tiempo sola).
    ['sleep', resting ? (sleepy ? 70 : 15) : sleepy ? 10 : 0],
  ];
  // Evita repetir sentarse/acostarse/dormir seguido.
  const filtered = options.filter(([a]) => !(a === s.activity && (a === 'sit' || a === 'sleep' || a === 'lie')));
  const total = filtered.reduce((acc, [, w]) => acc + w, 0);
  let r = ctx.rng() * total;
  let pick: Activity = 'idle';
  for (const [a, w] of filtered) {
    if ((r -= w) < 0) {
      pick = a;
      break;
    }
  }
  if (pick === 'walk' || pick === 'run') {
    return lasting(pick, now, ctx, s.lastInteraction, ctx.pickTarget(pick === 'run'));
  }
  return lasting(pick, now, ctx, s.lastInteraction);
}

export function step(s: BrainState, e: BrainEvent, ctx: BrainContext): BrainState {
  switch (e.type) {
    case 'tick': {
      if (e.now < s.until) return s;
      if (s.activity === 'wake') return lasting('sit', e.now, ctx, s.lastInteraction);
      if (s.activity === 'oops') return lasting('sad', e.now, ctx, s.lastInteraction);
      return chooseNext(s, e.now, ctx);
    }
    case 'arrived':
      if (s.activity !== 'walk' && s.activity !== 'run') return s;
      return lasting(ctx.rng() < 0.5 ? 'sit' : 'idle', e.now, ctx, s.lastInteraction);
    case 'click': {
      const touched = { ...s, lastInteraction: e.now };
      if (s.activity === 'dragged') return touched;
      if (s.activity === 'sleep') return lasting('wake', e.now, ctx, e.now);
      if (HELD.includes(s.activity)) return touched; // no interrumpe una respuesta en curso
      if (e.count >= 3) return lasting('celebrate', e.now, ctx, e.now);
      return lasting('happy', e.now, ctx, e.now);
    }
    case 'dragStart':
      return lasting('dragged', e.now, ctx, e.now);
    case 'dragEnd':
      return s.activity === 'dragged' ? lasting('idle', e.now, ctx, e.now) : s;
    case 'mood': {
      if (s.activity === 'dragged') return { ...s, lastInteraction: e.now };
      if (e.mood === 'idle') {
        return HELD.includes(s.activity) ? lasting('idle', e.now, ctx, e.now) : { ...s, lastInteraction: e.now };
      }
      // Un error primero la sorprende (saltito) y luego la pone triste.
      if (e.mood === 'sad') return lasting('oops', e.now, ctx, e.now);
      return lasting(e.mood, e.now, ctx, e.now);
    }
    case 'noticed':
      if (s.activity === 'idle' || s.activity === 'sit' || s.activity === 'lie') {
        return lasting('curious', e.now, ctx, e.now);
      }
      return s;
    case 'command': {
      if (e.activity === 'walk') return lasting('walk', e.now, ctx, e.now, ctx.pickTarget(false));
      return lasting(e.activity, e.now, ctx, e.now);
    }
  }
}

/** Velocidad de desplazamiento en píxeles lógicos por segundo. */
export function speedFor(a: Activity): number {
  return a === 'run' ? 150 : a === 'walk' ? 45 : 0;
}
