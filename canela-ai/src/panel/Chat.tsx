import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  canChat,
  onMemoryWarning,
  PROVIDER_NAMES,
  data,
  formatUsd,
  imageUrl,
  onJobFinished,
  onJobsChanged,
  sendMood,
  type ConversationMeta,
  type ImageData,
  type JobInfo,
  type KeyStatus,
  type Settings,
  type StreamEvent,
} from "../lib/ipc";
import {
  type Attachment,
  bubbleSnippet,
  compactAt,
  compactWith,
  composeUserText,
  conversationCost,
  type Conversation,
  conversationTitle,
  newId,
  type PendingConfirm,
  toWire,
  type UiMessage,
  wireSize,
} from "./chatModel";
import { CaptureSheet } from "./CaptureSheet";
import { ClipIcon, MicIcon, ScreenIcon, SendIcon, StopIcon } from "./icons";
import { Markdown } from "./markdown";
import { useVoice } from "./useVoice";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Props {
  settings: Settings;
  keys: KeyStatus;
  onOpenSettings: () => void;
}

const SUGGESTIONS = [
  "¿Qué tengo que hacer hoy?",
  "Abre Visual Studio Code",
  "Recuerda que estoy desarrollando una página web",
];

function DiffView({ diff }: { diff: string }) {
  return (
    <pre className="diff">
      {diff.split("\n").map((line, i) => (
        <span
          key={i}
          className={
            line.startsWith("+") && !line.startsWith("+++")
              ? "add"
              : line.startsWith("-") && !line.startsWith("---")
                ? "del"
                : ""
          }
        >
          {line + "\n"}
        </span>
      ))}
    </pre>
  );
}

function ConfirmCard({
  c,
  onAnswer,
}: {
  c: PendingConfirm;
  onAnswer: (ok: boolean) => void;
}) {
  if (c.state !== "waiting") {
    // Una vez respondida, se reduce a una línea para no recargar el chat.
    return (
      <div className="confirm-done">
        {c.title} · {c.state === "approved" ? "Permitido" : "No permitido"}
      </div>
    );
  }
  return (
    <div className={`confirm ${c.state}`}>
      <div className="confirm-title">{c.title}</div>
      {c.details && <div className="confirm-details">{c.details}</div>}
      {c.image && (
        <img
          className="confirm-image"
          src={c.image}
          alt="Captura que se enviaría"
        />
      )}
      {c.diff && <DiffView diff={c.diff} />}
      {c.state === "waiting" ? (
        <div className="row end">
          <button className="btn" onClick={() => onAnswer(false)}>
            No
          </button>
          <button className="btn primary" onClick={() => onAnswer(true)}>
            Permitir
          </button>
        </div>
      ) : (
        <div className="confirm-state">
          {c.state === "approved" ? "Permitido" : "No permitido"}
        </div>
      )}
    </div>
  );
}

export function Chat({ settings, keys, onOpenSettings }: Props) {
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [input, setInput] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [images, setImages] = useState<ImageData[]>([]);
  const [showCapture, setShowCapture] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<JobInfo[]>([]);
  const [showJobs, setShowJobs] = useState(false);
  const [history, setHistory] = useState<ConversationMeta[] | null>(null);
  const cancelRef = useRef<(() => Promise<void>) | null>(null);
  const convRef = useRef<{
    id: string;
    userName: string;
    context: string;
  } | null>(null);
  const messagesRef = useRef<UiMessage[]>([]);
  const listRef = useRef<HTMLDivElement>(null);
  const listenTimer = useRef<number>(undefined);
  const sendRef = useRef<(text?: string, spoken?: boolean) => Promise<void>>(
    async () => undefined,
  );
  messagesRef.current = messages;
  const voiceCtl = useVoice(
    settings,
    (text) => void sendRef.current(text, true),
  );

  const hasKey = canChat(settings.provider, keys);
  const providerName = PROVIDER_NAMES[settings.provider];
  const isLocal = settings.provider === "ollama";
  // Con Ollama: comprobar al abrir si está encendido y el modelo instalado.
  const [ollamaProblem, setOllamaProblem] = useState<string | null>(null);
  // Avisos de RAM baja (del registro de memoria).
  const [memWarning, setMemWarning] = useState<string | null>(null);
  useEffect(() => {
    const un = onMemoryWarning((e) => {
      setMemWarning(e ? e.message : null);
      if (e)
        void sendMood({
          mood: "sad",
          bubble: "¡Guau! Tu PC se está quedando sin memoria libre.",
        });
    });
    return () => void un.then((f) => f());
  }, []);
  useEffect(() => {
    if (!isLocal) {
      setOllamaProblem(null);
      return;
    }
    let alive = true;
    void api
      .ollamaStatus()
      .then(
        (st) =>
          alive &&
          setOllamaProblem(st.running && st.modelInstalled ? null : st.message),
      )
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [isLocal, settings.ollamaModel, settings.ollamaUrl]);
  const activeJobs = jobs.filter(
    (j) => j.status === "queued" || j.status === "running",
  );

  useEffect(() => {
    if (messages.length > 0)
      listRef.current?.scrollTo({
        top: listRef.current.scrollHeight,
        behavior: "smooth",
      });
  }, [messages]);

  const persist = useCallback(
    (msgs: UiMessage[]) => {
      const conv = convRef.current;
      if (!settings.saveConversations || !conv || msgs.length === 0) return;
      const payload: Conversation = {
        v: 1,
        id: conv.id,
        userName: conv.userName,
        context: conv.context,
        messages: msgs.map((m) => ({ ...m, confirms: undefined })),
      };
      void data
        .conversationSave(
          conv.id,
          conversationTitle(msgs),
          JSON.stringify(payload),
        )
        .catch(() => undefined);
    },
    [settings.saveConversations],
  );

  // Trabajos en segundo plano: estado y resultados.
  useEffect(() => {
    void api.jobsList().then(setJobs);
    const un = [
      onJobsChanged(setJobs),
      onJobFinished((job) => {
        const ok = job.status === "done";
        const msg: UiMessage = {
          id: newId(),
          role: "assistant",
          job: true,
          status: "done",
          text: ok
            ? `**${job.title}** — terminado.\n\n${job.result ?? ""}`
            : `**${job.title}** — no se pudo completar: ${job.error ?? "error desconocido"}`,
        };
        setMessages((prev) => {
          const next = [...prev, msg];
          persist(next);
          return next;
        });
        void sendMood(
          ok
            ? { mood: "celebrate", bubble: `¡Terminé: ${job.title}! 🐾` }
            : {
                mood: "sad",
                bubble: "Un trabajo no salió bien. Mira el chat.",
              },
        );
      }),
    ];
    return () => un.forEach((p) => void p.then((f) => f()));
  }, [persist]);

  const patch = (id: string, f: (m: UiMessage) => UiMessage) =>
    setMessages((prev) => prev.map((m) => (m.id === id ? f(m) : m)));

  async function ensureConversation() {
    if (convRef.current) return convRef.current;
    const context = await data.conversationContext().catch(() => "");
    convRef.current = { id: newId(), userName: settings.userName, context };
    return convRef.current;
  }

  async function send(textOverride?: string, spoken = false) {
    const text =
      composeUserText(textOverride ?? input, attachments) ||
      (images.length ? "¿Qué ves en esta imagen?" : "");
    if (!text || busyId) return;
    if (!hasKey) {
      onOpenSettings();
      return;
    }
    // Nombre y contexto (memoria, fecha) se fijan al empezar la conversación
    // para que el prompt de sistema no cambie a mitad de ella.
    const conv = await ensureConversation();

    const user: UiMessage = {
      id: newId(),
      role: "user",
      text,
      status: "done",
      attachments: attachments.map((a) => a.name),
      images: images.length ? images : undefined,
    };
    const reply: UiMessage = {
      id: newId(),
      role: "assistant",
      text: "",
      status: "streaming",
      thinking: true,
      provider: settings.provider,
      tools: [],
      confirms: [],
    };
    // Conversaciones muy largas: resume lo antiguo para no gastar de más.
    let base = messagesRef.current;
    if (
      wireSize([...base, user]) >
        compactAt(settings.provider, settings.ollamaNumCtx) &&
      base.length > 8
    ) {
      setBusyId(reply.id);
      try {
        const summary = await api.summarize(
          toWire(base.slice(0, -6)),
          conv.userName,
        );
        base = compactWith(base, summary, newId());
        setMessages(base);
      } catch {
        /* si falla el resumen, se envía completo */
      }
    }
    const wire = toWire([...base, user]);
    setMessages((prev) => [...prev, user, reply]);
    setInput("");
    setAttachments([]);
    setImages([]);
    setBusyId(reply.id);
    window.clearTimeout(listenTimer.current);
    void sendMood({ mood: "think" });

    let acc = "";
    let lastBubble = 0;
    // Lectura en voz alta mientras llega la respuesta (si corresponde).
    const speech = voiceCtl.speechStream(spoken);
    const finish = (f: (m: UiMessage) => UiMessage) => {
      setMessages((prev) => {
        const next = prev.map((m) => (m.id === reply.id ? f(m) : m));
        persist(next);
        return next;
      });
      setBusyId(null);
    };

    const onEvent = (ev: StreamEvent) => {
      switch (ev.type) {
        case "started":
          patch(reply.id, (m) => ({ ...m, model: ev.model }));
          break;
        case "thinking":
          patch(reply.id, (m) => ({ ...m, thinking: true }));
          break;
        case "delta": {
          acc += ev.text;
          speech?.feed(ev.text);
          patch(reply.id, (m) => ({
            ...m,
            text: m.text + ev.text,
            thinking: false,
          }));
          const now = Date.now();
          if (acc.trim() && now - lastBubble > 700) {
            lastBubble = now;
            void sendMood({ mood: "talk", bubble: bubbleSnippet(acc) });
          }
          break;
        }
        case "toolStart":
          patch(reply.id, (m) => ({
            ...m,
            thinking: true,
            tools: [
              ...(m.tools ?? []),
              { callId: ev.callId, label: ev.label, status: "running" },
            ],
          }));
          void sendMood({ mood: "think" });
          break;
        case "toolEnd":
          patch(reply.id, (m) => {
            const exists = m.tools?.some((t) => t.callId === ev.callId);
            const tools = exists
              ? (m.tools ?? []).map((t) =>
                  t.callId === ev.callId
                    ? {
                        ...t,
                        status: ev.ok ? ("ok" as const) : ("error" as const),
                        summary: ev.ok ? undefined : ev.summary,
                      }
                    : t,
                )
              : [
                  ...(m.tools ?? []),
                  {
                    callId: ev.callId,
                    label: ev.summary,
                    status: ev.ok ? ("ok" as const) : ("error" as const),
                  },
                ];
            return { ...m, tools };
          });
          break;
        case "confirmRequest":
          patch(reply.id, (m) => ({
            ...m,
            thinking: false,
            confirms: [
              ...(m.confirms ?? []),
              {
                confirmId: ev.confirmId,
                title: ev.title,
                details: ev.details,
                diff: ev.diff,
                image: ev.image,
                state: "waiting",
              },
            ],
          }));
          void sendMood({
            mood: "listen",
            bubble: "Necesito tu permiso para continuar 🐾",
          });
          // Si hablaste con el panel cerrado, lo abre para que puedas responder.
          void getCurrentWindow()
            .isVisible()
            .then((visible) => {
              if (!visible) void api.showPanel();
            });
          break;
        case "done":
          finish((m) => ({
            ...m,
            text: m.text.trim(),
            status: "done",
            thinking: false,
            model: ev.model ?? m.model,
            notice: ev.notice ?? undefined,
            provider: ev.provider,
            transcript: ev.transcript,
            usage: ev.usage,
            costUsd: ev.costUsd,
          }));
          if (speech) speech.end();
          void sendMood(
            speech
              ? {
                  mood: "talk",
                  bubble: acc.trim() ? bubbleSnippet(acc) : undefined,
                }
              : {
                  mood: ev.stopReason === "refusal" ? "sad" : "happy",
                  bubble: acc.trim()
                    ? bubbleSnippet(acc)
                    : (ev.notice ?? undefined),
                },
          );
          break;
        case "error":
          voiceCtl.stopSpeaking();
          finish((m) => ({
            ...m,
            status: "error",
            thinking: false,
            notice: ev.message,
          }));
          void sendMood({
            mood: "sad",
            bubble: "Ups… algo salió mal. Mira el panel 🐾",
          });
          break;
        case "cancelled":
          finish((m) => ({
            ...m,
            status: "cancelled",
            thinking: false,
            notice: "Respuesta detenida.",
            confirms: (m.confirms ?? []).map((c) =>
              c.state === "waiting" ? { ...c, state: "denied" } : c,
            ),
          }));
          void sendMood({ mood: "idle" });
          break;
      }
    };

    try {
      cancelRef.current = await api.chat(
        reply.id,
        wire,
        conv.userName,
        conv.context,
        onEvent,
      );
    } catch (e) {
      finish((m) => ({
        ...m,
        status: "error",
        thinking: false,
        notice: String(e),
      }));
      void sendMood({ mood: "sad", bubble: String(e) });
    }
  }

  async function answer(msgId: string, confirmId: string, ok: boolean) {
    patch(msgId, (m) => ({
      ...m,
      thinking: ok,
      confirms: (m.confirms ?? []).map((c) =>
        c.confirmId === confirmId
          ? { ...c, state: ok ? "approved" : "denied" }
          : c,
      ),
    }));
    await api.confirmAction(confirmId, ok);
    void sendMood({ mood: "think" });
  }

  async function stop() {
    voiceCtl.stopSpeaking();
    await cancelRef.current?.();
  }
  sendRef.current = send;

  async function newConversation() {
    if (busyId) await stop();
    setMessages([]);
    convRef.current = null;
    setHistory(null);
    void sendMood({ mood: "idle" });
  }

  async function openConversation(id: string) {
    const raw = await data.conversationGet(id);
    if (!raw) return;
    try {
      const conv = JSON.parse(raw) as Conversation;
      if (busyId) await stop();
      convRef.current = {
        id: conv.id,
        userName: conv.userName,
        context: conv.context,
      };
      setMessages(
        conv.messages.map((m) =>
          m.status === "streaming" ? { ...m, status: "cancelled" } : m,
        ),
      );
      setHistory(null);
    } catch {
      setFileError("No pude abrir esa conversación.");
    }
  }

  /** Selector de archivos de Windows: el texto se extrae en tu equipo. */
  async function pickFiles() {
    setFileError(null);
    const picked = await openDialog({
      multiple: true,
      title: "Adjuntar archivos",
      filters: [
        {
          name: "Documentos, imágenes y código",
          extensions: [
            "pdf",
            "docx",
            "png",
            "jpg",
            "jpeg",
            "webp",
            "txt",
            "md",
            "csv",
            "json",
            "log",
            "js",
            "jsx",
            "ts",
            "tsx",
            "py",
            "rs",
            "java",
            "cs",
            "cpp",
            "c",
            "h",
            "html",
            "css",
            "sql",
            "yaml",
            "yml",
            "toml",
            "xml",
          ],
        },
        { name: "Todos los archivos", extensions: ["*"] },
      ],
    });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    for (const path of paths) {
      try {
        const a = await api.prepareAttachment(path);
        if (a.kind === "image" && a.image)
          setImages((prev) => [...prev, a.image!].slice(0, 4));
        else if (a.text !== null)
          setAttachments((prev) => [
            ...prev,
            { name: a.name, content: a.text ?? "" },
          ]);
        if (a.note) setFileError(a.note);
      } catch (e) {
        setFileError(String(e));
      }
    }
  }

  function onTyping(value: string) {
    setInput(value);
    if (busyId) return;
    window.clearTimeout(listenTimer.current);
    void sendMood({ mood: "listen" });
    listenTimer.current = window.setTimeout(
      () => void sendMood({ mood: "idle" }),
      4000,
    );
  }

  return (
    <div className="chat">
      <div className="chat-bar">
        <span>
          {providerName}
          {isLocal && <span className="cost"> · en este equipo, gratis</span>}
          {!isLocal && conversationCost(messages) !== null && (
            <span className="cost">
              {" "}
              · conversación ≈ {formatUsd(conversationCost(messages)!)}
            </span>
          )}
        </span>
        <div className="chat-bar-actions">
          {activeJobs.length > 0 && (
            <button
              className="text-button busy"
              onClick={() => setShowJobs(!showJobs)}
            >
              {activeJobs.length === 1
                ? "1 tarea en segundo plano"
                : `${activeJobs.length} tareas en segundo plano`}
            </button>
          )}
          {settings.saveConversations && (
            <button
              className="text-button"
              onClick={() =>
                history
                  ? setHistory(null)
                  : void data.conversationList().then(setHistory)
              }
            >
              Historial
            </button>
          )}
          {messages.length > 0 && (
            <button
              className="text-button"
              onClick={() => void newConversation()}
            >
              Nueva
            </button>
          )}
        </div>
      </div>

      {showJobs && jobs.length > 0 && (
        <div className="popover">
          {jobs
            .slice()
            .reverse()
            .slice(0, 8)
            .map((j) => (
              <div key={j.id} className="job">
                <div>
                  <div className="job-title">{j.title}</div>
                  <div className="job-status">
                    {
                      {
                        queued: "En cola",
                        running: j.progress ?? "Trabajando…",
                        done: "Terminado",
                        failed: "Falló",
                        cancelled: "Cancelado",
                      }[j.status]
                    }
                  </div>
                </div>
                {(j.status === "queued" || j.status === "running") && (
                  <button
                    className="text-button"
                    onClick={() => void api.jobCancel(j.id)}
                  >
                    Cancelar
                  </button>
                )}
              </div>
            ))}
        </div>
      )}

      {history && (
        <div className="popover history">
          {history.length === 0 && (
            <p className="muted">No hay conversaciones guardadas.</p>
          )}
          {history.map((h) => (
            <div key={h.id} className="history-item">
              <button
                className="history-open"
                onClick={() => void openConversation(h.id)}
              >
                <span>{h.title}</span>
                <small>{h.updatedAt.slice(0, 16)}</small>
              </button>
              <button
                className="icon-btn"
                aria-label="Borrar conversación"
                onClick={() =>
                  void data
                    .conversationDelete(h.id)
                    .then(() =>
                      setHistory(history.filter((x) => x.id !== h.id)),
                    )
                }
              >
                ✕
              </button>
            </div>
          ))}
        </div>
      )}

      {memWarning && (
        <div className="banner">
          ⚠ {memWarning}
          <button
            className="btn"
            onClick={() =>
              void api.memoryRelease().then((r) => setMemWarning(r.message))
            }
          >
            Liberar memoria de IA
          </button>
          <button className="btn" onClick={() => setMemWarning(null)}>
            Cerrar
          </button>
        </div>
      )}

      {ollamaProblem && (
        <div className="banner">
          {ollamaProblem}
          <button
            className="btn"
            onClick={() =>
              void api
                .ollamaStatus()
                .then((st) =>
                  setOllamaProblem(
                    st.running && st.modelInstalled ? null : st.message,
                  ),
                )
            }
          >
            Reintentar
          </button>
        </div>
      )}

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
            <img
              src="/sprites/canela-face.png"
              alt=""
              className="empty-face"
              onError={(e) => (e.currentTarget.style.display = "none")}
            />
            <h2>
              Hola{settings.userName ? `, ${settings.userName}` : ""}. Soy
              Canela.
            </h2>
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
        {messages.map((m) =>
          m.summary ? (
            <details key={m.id} className="summary-note">
              <summary>
                Resumí la parte anterior de la conversación para ahorrar espacio
              </summary>
              <p className="md-text">
                {m.text.replace(
                  "[Resumen de nuestra conversación anterior]\n",
                  "",
                )}
              </p>
            </details>
          ) : (
            <div
              key={m.id}
              className={`msg ${m.role} ${m.status} ${m.job ? "job-msg" : ""}`}
            >
              {m.role === "assistant" && (
                <div className="msg-author">
                  Canela
                  {m.provider === "ollama" && (
                    <span
                      className="cost"
                      title={
                        m.usage
                          ? `${m.usage.input} tokens de entrada · ${m.usage.output} de salida`
                          : ""
                      }
                    >
                      {" "}
                      · local
                    </span>
                  )}
                  {m.provider !== "ollama" && typeof m.costUsd === "number" && (
                    <span
                      className="cost"
                      title={
                        m.usage
                          ? `${m.usage.input + m.usage.cacheRead + m.usage.cacheWrite} tokens de entrada · ${m.usage.output} de salida`
                          : ""
                      }
                    >
                      {" "}
                      · ≈ {formatUsd(m.costUsd)}
                    </span>
                  )}
                </div>
              )}
              {m.attachments && m.attachments.length > 0 && (
                <div className="msg-files">
                  Adjunto: {m.attachments.join(", ")}
                </div>
              )}
              {m.images && m.images.length > 0 && (
                <div className="msg-images">
                  {m.images.map((im, i) => (
                    <img key={i} src={imageUrl(im)} alt="Imagen enviada" />
                  ))}
                </div>
              )}
              {m.tools && m.tools.length > 0 && (
                <ul className="tool-log">
                  {m.tools.map((t) => (
                    <li key={t.callId} className={t.status}>
                      <span className="tool-icon">
                        {t.status === "running"
                          ? "…"
                          : t.status === "ok"
                            ? "✓"
                            : "✕"}
                      </span>
                      {t.label}
                      {t.summary && (
                        <span className="tool-why"> — {t.summary}</span>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {m.confirms?.map((c) => (
                <ConfirmCard
                  key={c.confirmId}
                  c={c}
                  onAnswer={(ok) => void answer(m.id, c.confirmId, ok)}
                />
              ))}
              {(m.text || m.role === "user") && (
                <div className="msg-body">
                  {m.role === "user" ? (
                    <p className="md-text">
                      {m.attachments?.length
                        ? m.text.split("\n\n[Archivo adjunto:")[0]
                        : m.text}
                    </p>
                  ) : (
                    <Markdown text={m.text} />
                  )}
                </div>
              )}
              {m.thinking && m.status === "streaming" && (
                <div className="typing">
                  <span />
                  <span />
                  <span />
                  {m.text ? null : <em>Canela está pensando…</em>}
                </div>
              )}
              {m.notice && (
                <div className={`notice ${m.status === "error" ? "err" : ""}`}>
                  {m.notice}
                </div>
              )}
            </div>
          ),
        )}
      </div>

      {showCapture && (
        <CaptureSheet
          onAttach={(img) => setImages((prev) => [...prev, img].slice(0, 4))}
          onClose={() => setShowCapture(false)}
        />
      )}

      {images.length > 0 && (
        <div className="attachments">
          {images.map((im, i) => (
            <span key={i} className="chip thumb">
              <img src={imageUrl(im)} alt="" />
              Captura {im.width}×{im.height}
              <button
                onClick={() =>
                  setImages((prev) => prev.filter((_, j) => j !== i))
                }
                aria-label="Quitar captura"
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}

      {attachments.length > 0 && (
        <div className="attachments">
          {attachments.map((a, i) => (
            <span key={i} className="chip">
              {a.name}
              <button
                onClick={() =>
                  setAttachments((prev) => prev.filter((_, j) => j !== i))
                }
                aria-label="Quitar"
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}
      {fileError && <div className="notice err slim">{fileError}</div>}

      {(voiceCtl.phase !== "idle" || voiceCtl.message) && (
        <div className={`voice-status ${voiceCtl.phase}`}>
          {voiceCtl.phase === "listening" && (
            <>
              <span className="rec-dot" />
              Escuchando… habla y haz una pausa para terminar
              <span className="level">
                <span
                  style={{ width: `${Math.min(100, voiceCtl.level * 900)}%` }}
                />
              </span>
            </>
          )}
          {voiceCtl.phase === "processing" && "Entendiendo lo que dijiste…"}
          {voiceCtl.phase === "speaking" && (
            <>
              Canela está hablando
              <button className="text-button" onClick={voiceCtl.stopSpeaking}>
                Callar
              </button>
            </>
          )}
          {voiceCtl.phase === "idle" && voiceCtl.message && (
            <>
              <span className="voice-msg">{voiceCtl.message}</span>
              <button
                className="text-button"
                onClick={() => voiceCtl.setMessage(null)}
              >
                Cerrar
              </button>
            </>
          )}
        </div>
      )}

      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <button
          type="button"
          className="icon"
          title="Adjuntar PDF, Word, imagen o texto"
          aria-label="Adjuntar archivo"
          onClick={() => void pickFiles()}
        >
          <ClipIcon />
        </button>
        {settings.visionEnabled && (
          <button
            type="button"
            className="icon"
            title="Compartir una captura de pantalla"
            aria-label="Compartir captura"
            onClick={() => setShowCapture(true)}
          >
            <ScreenIcon />
          </button>
        )}
        <textarea
          value={input}
          placeholder={
            voiceCtl.phase === "listening"
              ? "Te escucho…"
              : hasKey
                ? "Escribe un mensaje…"
                : "Configura tu clave en Ajustes"
          }
          onChange={(e) => onTyping(e.target.value)}
          onBlur={() =>
            !busyId &&
            voiceCtl.phase === "idle" &&
            void sendMood({ mood: "idle" })
          }
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
          rows={1}
        />
        <button
          type="button"
          className={`icon mic ${voiceCtl.phase}`}
          title={
            voiceCtl.phase === "listening"
              ? "Terminar (Ctrl+Alt+V)"
              : "Hablar con Canela (Ctrl+Alt+V)"
          }
          aria-label="Micrófono"
          aria-pressed={voiceCtl.phase === "listening"}
          onClick={() => void voiceCtl.toggle()}
          disabled={!hasKey && voiceCtl.phase === "idle"}
        >
          <MicIcon />
        </button>
        {busyId || voiceCtl.phase === "speaking" ? (
          <button
            type="button"
            className="stop"
            onClick={() => void stop()}
            title="Detener"
            aria-label="Detener"
          >
            <StopIcon />
          </button>
        ) : (
          <button
            type="submit"
            className="send"
            disabled={
              !input.trim() && attachments.length === 0 && images.length === 0
            }
            title="Enviar (Enter)"
            aria-label="Enviar"
          >
            <SendIcon />
          </button>
        )}
      </form>
    </div>
  );
}
