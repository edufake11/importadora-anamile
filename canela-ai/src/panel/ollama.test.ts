import { describe, expect, it } from 'vitest';
import { canChat, needsKey, PROVIDER_NAMES } from '../lib/ipc';
import { compactAt, COMPACT_AT_CHARS } from './chatModel';
import { estimateVramGb } from './OllamaSettings';

describe('Ollama como proveedor', () => {
  it('no necesita clave', () => {
    const none = { anthropic: false, openai: false };
    expect(needsKey('ollama')).toBe(false);
    expect(canChat('ollama', none)).toBe(true);
    expect(canChat('anthropic', none)).toBe(false);
    expect(canChat('openai', { anthropic: false, openai: true })).toBe(true);
    expect(PROVIDER_NAMES.ollama).toBe('Ollama (local)');
  });

  it('resume antes con el contexto pequeño de un modelo local', () => {
    expect(compactAt('anthropic', 16384)).toBe(COMPACT_AT_CHARS);
    expect(compactAt('ollama', 16384)).toBe((16384 - 7000) * 3);
    expect(compactAt('ollama', 4096)).toBe(6000);
    expect(compactAt('ollama', 32768)).toBeGreaterThan(compactAt('ollama', 16384));
  });

  it('estima la memoria de la GPU', () => {
    expect(estimateVramGb(16384)).toBeCloseTo(7.6, 1);
    expect(estimateVramGb(32768)).toBeLessThan(16);
  });
});
