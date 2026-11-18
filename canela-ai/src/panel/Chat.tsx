import { useEffect, useRef, useState } from 'react';
import { api, sendMood, type KeyStatus, type Settings } from '../lib/ipc';
import {
  type Attachment,
  bubbleSnippet,
  composeUserText,
  isTextFile,
  MAX_ATTACHMENT_BYTES,
  newId,
  toWire,
  type UiMessage,
} from './chatModel';
import { Markdown } from './markdown';

interface Props {
  settings: Settings;
  keys: KeyStatus;
  onOpenSettings: () => void;
}

const SUGGESTIONS = ['Explícame un error de mi código', 'Ayúdame a organizar mi día', 'Resume un texto por mí'];

export function Chat({ settings, keys, onOpenSettings }: Props) {
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [input, setInput] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const cancelRef = useRef<(() => Promise<void>) | null>(null);
  const userNameRef = useRef<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const listenTimer = useRef<number>(undefined);

  const hasKey = settings.provider === 'anthropic' ? keys.anthropic : keys.openai;
  const providerName = settings.provider === 'anthropic' ? 'Claude' : 'ChatGPT';

  useEffect(() => {
    if (messages.length > 0) listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: 'smooth' });
  }, [messages]);

  const patch = (id: string, f: (m: UiMessage) => UiMessage) =>
    setMessages((prev) => prev.map((m) => (m.id === id ? f(m) : m)));

  async function send(textOverride?: string) {
    const text = composeUserText(textOverride ?? input, attachments);
    if (!text || busyId) return;
    if (!hasKey) {
      onOpenSettings();
      return;
    }
    // El nombre del usuario se fija al empezar la conversación para que el
    // prompt de sistema no cambie a mitad de ella.
    if (userNameRef.current === null) userNameRef.current = settings.userName;

    const user: UiMessage = {
      id: newId(),
      role: 'user',
      text,
      status: 'done',
      attachments: attachments.map((a) => a.name),
    };
    const reply: UiMessage = { id: newId(), role: 'assistant', text: '', status: 'streaming', thinking: true, provider: settings.provider };
    const wire = toWire([...messages, user]);
    setMessages((prev) => [...prev, user, reply]);
    setInput('');
    setAttachments([]);
    setBusyId(reply.id);
    window.clearTimeout(listenTimer.current);
    void sendMood({ mood: 'think' });

    let acc = '';
    let talking = false;
    let lastBubble = 0;
    try {
      cancelRef.current = await api.chat(reply.id, wire, userNameRef.current, (ev) => {
        switch (ev.type) {
          case 'started':
            patch(reply.id, (m) => ({ ...m, model: ev.model }));
            break;
          case 'thinking':
            patch(reply.id, (m) => ({ ...m, thinking: true }));
            break;
          case 'delta': {
            acc += ev.text;
            patch(reply.id, (m) => ({ ...m, text: m.text + ev.text, thinking: false }));
            const now = Date.now();
            if (!talking || now - lastBubble > 700) {
              talking = true;
              lastBubble = now;
              void sendMood({ mood: 'talk', bubble: bubbleSnippet(acc) });
            }
            break;
          }
          case 'done':
            patch(reply.id, (m) => ({
              ...m,
              status: 'done',
              thinking: false,
              model: ev.model ?? m.model,
              notice: ev.notice ?? undefined,
              anthropicContent: ev.anthropicContent ?? undefined,
            }));
            setBusyId(null);
            void sendMood({ mood: ev.stopReason === 'refusal' ? 'sad' : 'happy', bubble: acc ? bubbleSnippet(acc) : ev.notice ?? undefined });
            break;
          case 'error':
            patch(reply.id, (m) => ({ ...m, status: 'error', thinking: false, notice: ev.message }));
            setBusyId(null);
            void sendMood({ mood: 'sad', bubble: 'Ups… algo salió mal. Mira el panel 🐾' });
            break;
          case 'cancelled':
            patch(reply.id, (m) => ({ ...m, status: 'cancelled', thinking: false, notice: 'Respuesta detenida.' }));
            setBusyId(null);
            void sendMood({ mood: 'idle' });
            break;
        }
      });
    } catch (e) {
      patch(reply.id, (m) => ({ ...m, status: 'error', thinking: false, notice: String(e) }));
      setBusyId(null);
      void sendMood({ mood: 'sad', bubble: String(e) });
    }
  }

  async function stop() {
    await cancelRef.current?.();
  }

  async function newConversation() {
    if (busyId) await stop();
    setMessages([]);
    userNameRef.current = null;
    void sendMood({ mood: 'idle' });
  }

  async function addFiles(files: FileList | null) {
    setFileError(null);
    if (!files) return;
    const next: Attachment[] = [];
    for (const f of Array.from(files)) {
      if (!isTextFile(f.name)) {
        setFileError(`«${f.name}» no es un archivo de texto. Por ahora sólo se admiten archivos de texto o código.`);
        continue;
      }
      if (f.size > MAX_ATTACHMENT_BYTES) {
        setFileError(`«${f.name}» supera los ${MAX_ATTACHMENT_BYTES / 1024} KB.`);
        continue;
      }
      next.push({ name: f.name, content: await f.text() });
    }
    setAttachments((prev) => [...prev, ...next]);
    if (fileRef.current) fileRef.current.value = '';
  }

  function onTyping(value: string) {
    setInput(value);
    if (busyId) return;
    window.clearTimeout(listenTimer.current);
    void sendMood({ mood: 'listen' });
    listenTimer.current = window.setTimeout(() => void sendMood({ mood: 'idle' }), 4000);
  }

  return (
    <div className="chat">
      <div className="chat-bar">
        <span>{providerName}</span>
        {messages.length > 0 && (
          <button className="text-button" onClick={newConversation}>
            Nueva conversación
          </button>
        )}
      </div>

      {!hasKey && (
        <div className="banner">
          Añade tu clave de {providerName} para empezar.
          <button className="btn primary" onClick={onOpenSettings}>
            Configurar
          </button>
        </div>
      )}

      <div className="messages" ref={listRef}>
        {messages.length === 0 && (
          <div className="empty">
            <img src="/sprites/canela-face.png" alt="" className="empty-face" onError={(e) => (e.currentTarget.style.display = 'none')} />
            <h2>Hola{settings.userName ? `, ${settings.userName}` : ''}. Soy Canela.</h2>
            <p>¿En qué te ayudo hoy?</p>
            <div className="suggestions">
              {SUGGESTIONS.map((s) => (
                <button key={s} onClick={() => void send(s)} disabled={!hasKey}>
                  {s}
                </button>
              ))}
            </div>
          </div>
        )}
        {messages.map((m) => (
          <div key={m.id} className={`msg ${m.role} ${m.status}`}>
            {m.role === 'assistant' && <div className="msg-author">Canela</div>}
            {m.attachments && m.attachments.length > 0 && (
              <div className="msg-files">Adjunto: {m.attachments.join(', ')}</div>
            )}
            <div className="msg-body">
              {m.role === 'user' ? (
                <p className="md-text">{m.attachments?.length ? m.text.split('\n\n[Archivo adjunto:')[0] : m.text}</p>
              ) : (
                <Markdown text={m.text} />
              )}
              {m.thinking && m.status === 'streaming' && (
                <div className="typing">
                  <span />
                  <span />
                  <span />
                  {m.text ? null : <em>Canela está pensando…</em>}
                </div>
              )}
            </div>
            {m.notice && <div className={`notice ${m.status === 'error' ? 'err' : ''}`}>{m.notice}</div>}
          </div>
        ))}
      </div>

      {attachments.length > 0 && (
        <div className="attachments">
          {attachments.map((a, i) => (
            <span key={i} className="chip">
              {a.name}
              <button onClick={() => setAttachments((prev) => prev.filter((_, j) => j !== i))} aria-label="Quitar">
                ×
              </button>
            </span>
          ))}
        </div>
      )}
      {fileError && <div className="notice err slim">{fileError}</div>}

      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <button type="button" className="icon" title="Adjuntar archivo de texto o código" aria-label="Adjuntar" onClick={() => fileRef.current?.click()}>
          +
        </button>
        <input ref={fileRef} type="file" multiple hidden onChange={(e) => void addFiles(e.target.files)} />
        <textarea
          value={input}
          placeholder={hasKey ? 'Escribe un mensaje…' : 'Configura tu clave en Ajustes'}
          onChange={(e) => onTyping(e.target.value)}
          onBlur={() => !busyId && void sendMood({ mood: 'idle' })}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
          rows={1}
        />
        {busyId ? (
          <button type="button" className="stop" onClick={() => void stop()}>
            Detener
          </button>
        ) : (
          <button type="submit" className="send" disabled={!input.trim() && attachments.length === 0} title="Enviar (Enter)" aria-label="Enviar">
            ↑
          </button>
        )}
      </form>
    </div>
  );
}
