import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { frameIndex, REQUIRED_ANIMATIONS, type SpriteManifest, validateManifest } from './sprites';

const root = resolve(__dirname, '../../public/sprites');
const manifest = JSON.parse(readFileSync(resolve(root, 'canela.json'), 'utf8')) as SpriteManifest;

function pngSize(path: string) {
  const b = readFileSync(path);
  expect(b.subarray(1, 4).toString()).toBe('PNG');
  return { w: b.readUInt32BE(16), h: b.readUInt32BE(20) };
}

describe('spritesheet de Canela', () => {
  it('el manifiesto incluido es válido y coincide con la imagen', () => {
    const { w, h } = pngSize(resolve(root, manifest.image));
    expect(validateManifest(manifest, w, h)).toEqual([]);
  });

  it('tiene todas las animaciones requeridas', () => {
    for (const a of REQUIRED_ANIMATIONS) expect(manifest.animations[a]).toBeDefined();
  });

  it('detecta manifiestos rotos', () => {
    const broken: SpriteManifest = { ...manifest, animations: { idle: { row: 99, frames: 0, fps: 0, loop: true } } };
    const problems = validateManifest(broken, 192, 520);
    expect(problems.some((p) => p.includes('walk'))).toBe(true);
    expect(problems.some((p) => p.includes('fps'))).toBe(true);
    expect(problems.some((p) => p.includes('alto'))).toBe(true);
  });

  it('calcula fotogramas en bucle y sin bucle', () => {
    const loop = { row: 0, frames: 4, fps: 4, loop: true };
    expect(frameIndex(loop, 0, 0)).toBe(0);
    expect(frameIndex(loop, 0, 1000)).toBe(0);
    expect(frameIndex(loop, 0, 750)).toBe(3);
    const once = { ...loop, loop: false };
    expect(frameIndex(once, 0, 10_000)).toBe(3);
  });
});
