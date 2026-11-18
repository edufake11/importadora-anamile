// Modelo de datos del chat (puro, sin React) para poder probarlo.
import type { ProviderId, WireMessage } from '../lib/ipc';

export interface UiMessage {
  id: string;
  role: 'user' | 'assistant';
  text: string;
  status: 'streaming' | 'done' | 'error' | 'cancelled';
  anthropicContent?: unknown;
  provider?: ProviderId;
  model?: string;
  notice?: string;
  thinking?: boolean;
  /** Nombres de archivos adjuntos (sólo para mostrarlos). */
  attachments?: string[];
}

export interface Attachment {
  name: string;
  content: string;
}

export const MAX_ATTACHMENT_BYTES = 300 * 1024;
export const TEXT_EXTENSIONS = [
  'txt', 'md', 'markdown', 'log', 'csv', 'tsv', 'json', 'yaml', 'yml', 'toml', 'xml', 'html', 'css', 'scss',
  'js', 'jsx', 'ts', 'tsx', 'mjs', 'cjs', 'py', 'rs', 'go', 'java', 'kt', 'c', 'h', 'cpp', 'hpp', 'cs', 'php',
  'rb', 'swift', 'sql', 'sh', 'ps1', 'bat', 'ini', 'cfg', 'env.example', 'vue', 'svelte', 'dart', 'lua', 'r',
];

export function isTextFile(name: string): boolean {
  const lower = name.toLowerCase();
  return TEXT_EXTENSIONS.some((ext) => lower.endsWith('.' + ext)) || !lower.includes('.');
}

/** Texto final del mensaje del usuario con los adjuntos incrustados. */
export function composeUserText(text: string, attachments: Attachment[]): string {
  const parts = [text.trim()];
  for (const a of attachments) {
    const fence = a.content.includes('```') ? '~~~~' : '```';
    parts.push(`[Archivo adjunto: ${a.name}]\n${fence}\n${a.content}\n${fence}`);
  }
  return parts.filter(Boolean).join('\n\n');
}

/**
 * Historial que se envía al modelo: mensajes del usuario y respuestas
 * completas. Los errores no se envían; las respuestas canceladas sólo
 * conservan su texto (sin bloques internos incompletos).
 */
export function toWire(messages: UiMessage[]): WireMessage[] {
  const out: WireMessage[] = [];
  for (const m of messages) {
    if (m.role === 'user') {
      out.push({ role: 'user', text: m.text });
    } else if (m.status === 'done' || m.status === 'cancelled') {
      if (!m.text.trim() && !m.anthropicContent) continue;
      out.push({
        role: 'assistant',
        text: m.text,
        anthropicContent: m.status === 'done' ? m.anthropicContent ?? undefined : undefined,
      });
    }
  }
  return out;
}

/** Fragmento corto para la burbuja de Canela. */
export function bubbleSnippet(text: string, max = 140): string {
  const clean = text.replace(/```[\s\S]*?(```|$)/g, ' [código] ').replace(/\s+/g, ' ').trim();
  return clean.length > max ? clean.slice(0, max - 1).trimEnd() + '…' : clean;
}

export function newId(): string {
  return crypto.randomUUID();
}
