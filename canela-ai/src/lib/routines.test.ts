import { describe, expect, it } from 'vitest';
import { routineSteps } from './ipc';

describe('routineSteps', () => {
  it('lee los pasos guardados', () => {
    const steps = routineSteps({
      name: 'mañana',
      updatedAt: '',
      steps: JSON.stringify([{ herramienta: 'listar_tareas', parametros: {}, descripcion: 'Ver tareas' }, { nope: 1 }]),
    });
    expect(steps).toHaveLength(1);
    expect(steps[0].descripcion).toBe('Ver tareas');
  });

  it('no falla con datos dañados', () => {
    expect(routineSteps({ name: 'x', updatedAt: '', steps: '{roto' })).toEqual([]);
    expect(routineSteps({ name: 'x', updatedAt: '', steps: '{"a":1}' })).toEqual([]);
  });
});
