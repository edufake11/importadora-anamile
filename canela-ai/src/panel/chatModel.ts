// Modelo de datos del chat (puro, sin React) para poder probarlo.
import type { ProviderId, WireMessage } from '../lib/ipc';

export interface ToolActivity {
  callId: string;
  label: string;
  status: 'running' | 'ok' | 'error';
  summary?: string;
}

export interface PendingConfirm {
  confirmId: string;
  title: string;
  details: string;
  diff: string | null;
  state: 'waiting' | 'approved' | 'denied';
}

export interface UiMessage {
  id: string;
  role: 'user' | 'assistant';
  text: string;
  status: 'streaming' | 'done' | 'error' | 'cancelled';
  /** Mensajes originales del proveedor para reenviar (ver ai/mod.rs). */
  transcript?: unknown[];
  provider?: ProviderId;
  model?: string;
  notice?: string;
  thinking?: boolean;
  /** Nombres de archivos adjuntos (sólo para mostrarlos). */
  attachments?: string[];
  tools?: ToolActivity[];
  confirms?: PendingConfirm[];
  /** Mensaje generado al terminar un trabajo en segundo plano. */
  job?: boolean;
}

export interface Conversation {
  v: 1;
  id: string;
  userName: string;
  context: string;
  messages: UiMessage[];
}

export function conversationTitle(messages: UiMessage[]): string {
  const first = messages.find((m) => m.role === 'user')?.text ?? 'Conversación';
  const clean = first.split('\n\n[Archivo adjunto:')[0].replace(/\s+/g, ' ').trim();
  return clean.length > 60 ? clean.slice(0, 59) + '…' : clean || 'Conversación';
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
 * Historial que se envía al modelo: mensajes del usuario y respuestas.
 * Sólo las respuestas completas conservan su transcripción (razonamiento,
 * herramientas); las canceladas o con error envían únicamente su texto.
 */
export function toWire(messages: UiMessage[]): WireMessage[] {
  const out: WireMessage[] = [];
  for (const m of messages) {
    if (m.role === 'user') {
      out.push({ role: 'user', text: m.text });
      continue;
    }
    if (m.status === 'streaming') continue;
    const complete = m.status === 'done' && !!m.transcript?.length;
    if (!complete && !m.text.trim()) continue;
    out.push(
      complete
        ? { role: 'assistant', text: m.text, provider: m.provider, transcript: m.transcript }
        : { role: 'assistant', text: m.text.trim() },
    );
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
