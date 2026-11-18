import { describe, expect, it } from 'vitest';
import { type BrainContext, type BrainState, chooseNext, initialState, SLEEPY_AFTER_MS, speedFor, step } from './brain';

const ctx = (r = 0.5, wander = true): BrainContext => ({ rng: () => r, wander, pickTarget: () => ({ x: 10, y: 20 }) });

describe('máquina de estados de Canela', () => {
  it('empieza en idle', () => {
    expect(initialState(0).activity).toBe('idle');
  });

  it('no cambia antes de que termine la actividad', () => {
    const s = initialState(0);
    expect(step(s, { type: 'tick', now: 100 }, ctx())).toBe(s);
  });

  it('al terminar elige otra actividad', () => {
    const s = initialState(0);
    const n = step(s, { type: 'tick', now: s.until + 1 }, ctx(0.6));
    expect(n).not.toBe(s);
  });

  it('sin paseo libre nunca camina ni corre', () => {
    for (let r = 0; r < 1; r += 0.01) {
      const n = chooseNext(initialState(0), 1000, ctx(r, false));
      expect(['walk', 'run']).not.toContain(n.activity);
    }
  });

  it('caminar asigna un destino y al llegar se detiene', () => {
    const s = step(initialState(0), { type: 'command', activity: 'walk', now: 0 }, ctx());
    expect(s.activity).toBe('walk');
    expect(s.target).toEqual({ x: 10, y: 20 });
    expect(speedFor('walk')).toBeGreaterThan(0);
    const a = step(s, { type: 'arrived', now: 10 }, ctx(0.1));
    expect(['sit', 'idle']).toContain(a.activity);
    expect(speedFor(a.activity)).toBe(0);
  });

  it('con mucho tiempo sin interacción tiende a descansar', () => {
    const s: BrainState = { ...initialState(0), lastInteraction: 0 };
    const counts: Record<string, number> = {};
    for (let i = 0; i < 100; i++) {
      const n = chooseNext(s, SLEEPY_AFTER_MS + 1, ctx(i / 100));
      counts[n.activity] = (counts[n.activity] ?? 0) + 1;
    }
    expect((counts.sleep ?? 0) + (counts.lie ?? 0)).toBeGreaterThan(30);
  });

  it('un clic la despierta y luego se sienta', () => {
    const sleeping = step(initialState(0), { type: 'command', activity: 'sleep', now: 0 }, ctx());
    const waking = step(sleeping, { type: 'click', now: 5, count: 1 }, ctx());
    expect(waking.activity).toBe('wake');
    const after = step(waking, { type: 'tick', now: waking.until + 1 }, ctx());
    expect(after.activity).toBe('sit');
  });

  it('un clic la pone feliz y tres clics la hacen celebrar', () => {
    expect(step(initialState(0), { type: 'click', now: 1, count: 1 }, ctx()).activity).toBe('happy');
    expect(step(initialState(0), { type: 'click', now: 1, count: 3 }, ctx()).activity).toBe('celebrate');
  });

  it('pensar y hablar se mantienen hasta que el chat lo indique', () => {
    const t = step(initialState(0), { type: 'mood', mood: 'think', now: 0 }, ctx());
    expect(t.until).toBe(Infinity);
    expect(step(t, { type: 'tick', now: 1e9 }, ctx())).toBe(t);
    // un clic no interrumpe una respuesta en curso
    expect(step(t, { type: 'click', now: 1, count: 1 }, ctx()).activity).toBe('think');
    const talk = step(t, { type: 'mood', mood: 'talk', now: 2 }, ctx());
    expect(talk.activity).toBe('talk');
    expect(step(talk, { type: 'mood', mood: 'idle', now: 3 }, ctx()).activity).toBe('idle');
  });

  it('el arrastre tiene prioridad sobre los estados de ánimo', () => {
    const d = step(initialState(0), { type: 'dragStart', now: 0 }, ctx());
    expect(d.activity).toBe('dragged');
    expect(step(d, { type: 'mood', mood: 'talk', now: 1 }, ctx()).activity).toBe('dragged');
    expect(step(d, { type: 'dragEnd', now: 2 }, ctx()).activity).toBe('idle');
  });
});

describe('animaciones nuevas', () => {
  it('un error la sorprende y luego la pone triste', () => {
    const o = step(initialState(0), { type: 'mood', mood: 'sad', now: 0 }, ctx());
    expect(o.activity).toBe('oops');
    const s = step(o, { type: 'tick', now: o.until + 1 }, ctx());
    expect(s.activity).toBe('sad');
  });

  it('se pone curiosa si acercas el cursor mientras descansa', () => {
    expect(step(initialState(0), { type: 'noticed', now: 1 }, ctx()).activity).toBe('curious');
    const walking = step(initialState(0), { type: 'command', activity: 'walk', now: 0 }, ctx());
    expect(step(walking, { type: 'noticed', now: 1 }, ctx()).activity).toBe('walk');
  });

  it('sólo se duerme desde acostada salvo que lleve mucho tiempo sola', () => {
    for (let r = 0; r < 1; r += 0.01) {
      const n = chooseNext({ ...initialState(0), activity: 'idle' }, 1000, ctx(r));
      expect(n.activity).not.toBe('sleep');
    }
    const lying: BrainState = { ...initialState(0), activity: 'lie', lastInteraction: 0 };
    const counts: Record<string, number> = {};
    for (let i = 0; i < 100; i++) {
      const n = chooseNext(lying, SLEEPY_AFTER_MS + 1, ctx(i / 100));
      counts[n.activity] = (counts[n.activity] ?? 0) + 1;
    }
    expect(counts.sleep).toBeGreaterThan(40);
  });

  it('al despertar se estira y termina sentada', () => {
    const w = step({ ...initialState(0), activity: 'sleep', until: Infinity }, { type: 'click', now: 0, count: 1 }, ctx());
    expect(w.activity).toBe('wake');
    expect(w.until).toBe(1600);
  });
});

describe('reacciones a la voz', () => {
  it('escuchar → curiosa (procesando) → hablar → feliz → descanso', () => {
    let s = step(initialState(0), { type: 'mood', mood: 'listen', now: 0 }, ctx());
    expect(s.activity).toBe('listen');
    expect(step(s, { type: 'tick', now: 1e9 }, ctx())).toBe(s); // no se cansa de escuchar
    s = step(s, { type: 'mood', mood: 'curious', now: 1 }, ctx());
    expect(s.activity).toBe('curious');
    expect(s.until).toBe(Infinity); // se mantiene mientras procesa
    s = step(s, { type: 'mood', mood: 'talk', now: 2 }, ctx());
    expect(s.activity).toBe('talk');
    s = step(s, { type: 'mood', mood: 'happy', now: 3 }, ctx());
    expect(s.activity).toBe('happy');
    expect(s.until).toBeLessThan(Infinity); // la alegría dura un rato y vuelve a descansar
  });

  it('idle libera la curiosidad sostenida pero no la autónoma', () => {
    const held = step(initialState(0), { type: 'mood', mood: 'curious', now: 0 }, ctx());
    expect(step(held, { type: 'mood', mood: 'idle', now: 1 }, ctx()).activity).toBe('idle');
    const noticed = step(initialState(0), { type: 'noticed', now: 0 }, ctx());
    expect(noticed.external).toBeFalsy();
    expect(step(noticed, { type: 'mood', mood: 'idle', now: 1 }, ctx()).activity).toBe('curious');
  });
});
