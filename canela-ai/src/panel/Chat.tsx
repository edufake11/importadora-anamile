import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  canChat,
  routeAction,
  providerLabel,
  type CloudOffer,
  type ProviderId,
  onMemoryWarning,
  onWakeState,
  voice,
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
  type PlanStep,
  type Verdict,
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
  actionSummary,
  planReport,
  verdictView,
  PLAN_ICON,
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

/** Respuesta a la propuesta del enrutador. */
type RouteChoice = "local" | "cloud" | "cloud-always" | "cancel";

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
  // Enrutador: propuesta pendiente de usar un modelo externo.
  const [routeAsk, setRouteAsk] = useState<{
    offer: CloudOffer;
    localPossible: boolean;
    resolve: (c: RouteChoice) => void;
  } | null>(null);
  const [routeNotice, setRouteNotice] = useState<string | null>(null);
  // Principal externo + datos personales: autorización para ESTE mensaje.
  const [privacyAsk, setPrivacyAsk] = useState<{
    provider: ProviderId;
    summary: string;
    resolve: (ok: boolean) => void;
  } | null>(null);
  // «Permitir en esta conversación» (se reinicia con «Nueva»).
  const cloudAllowedRef = useRef(false);

  // Indicador de «Hola, Canela».
  const [wakeOn, setWakeOn] = useState(false);
  useEffect(() => {
    void voice
      .wakeStatus()
      .then(setWakeOn)
      .catch(() => undefined);
    const un = onWakeState((e) => setWakeOn(e.active));
    return () => void un.then((f) => f());
  }, []);
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
    // Enrutador (v0.6.1): modelo, privacidad y presupuesto.
    let override: ProviderId | undefined;
    let privacyAck: string[] | undefined;
    try {
      const prior = messagesRef.current;
      const d = await api.routeDecide({
        text,
        hasImages: images.length > 0,
        attachmentChars: attachments.reduce((n, a) => n + a.content.length, 0),
        historyChars: wireSize(prior),
        attachments: attachments.map((a) => a.content),
        history: prior.map((m) => m.text),
        historyImages: prior.reduce((n, m) => n + (m.images?.length ?? 0), 0),
      });
      const action = routeAction(d, cloudAllowedRef.current, settings.provider);
      if (action.kind === "blocked") {
        setRouteNotice(action.message);
        return;
      }
      if (action.kind === "ask") {
        const choice = await new Promise<RouteChoice>((resolve) =>
          setRouteAsk({
            offer: action.offer,
            localPossible: action.localPossible,
            resolve,
          }),
        );
        setRouteAsk(null);
        if (choice === "cancel") return;
        if (choice === "cloud-always") cloudAllowedRef.current = true;
        if (choice !== "local") {
          override = action.offer.provider;
          privacyAck = action.offer.ack;
        }
      } else if (action.kind === "confirm-send") {
        const ok = await new Promise<boolean>((resolve) =>
          setPrivacyAsk({ ...action, resolve }),
        );
        setPrivacyAsk(null);
        if (!ok) return;
        privacyAck = action.ack;
      } else {
        override = action.override;
      }
    } catch {
      /* si el enrutador falla, se usa el proveedor principal como antes */
    }
    setRouteNotice(null);

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
      provider: override ?? settings.provider,
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
        case "agent":
          patch(reply.id, (m) => ({
            ...m,
            agent: ev.id,
            agentName: ev.name,
            agentReason: ev.reason,
          }));
          break;
        case "toolVerified":
          patch(reply.id, (m) => ({
            ...m,
            tools: (m.tools ?? []).map((t) =>
              t.callId === ev.callId
                ? { ...t, verified: ev.ok, verifyDetail: ev.detail }
                : t,
            ),
          }));
          break;
        case "plan":
          if (ev.finished) {
            // El informe final se guarda aunque la respuesta se haya detenido.
            setMessages((prev) => {
              const next = prev.map((m) =>
                m.id === reply.id
                  ? { ...m, plan: ev.steps, planFinished: true }
                  : m,
              );
              persist(next);
              return next;
            });
          } else {
            patch(reply.id, (m) => ({ ...m, plan: ev.steps }));
          }
          break;
        case "report":
          patch(reply.id, (m) => ({
            ...m,
            actions: ev.actions,
            verdict: ev.verdict ?? undefined,
          }));
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
        override,
        privacyAck,
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
    cloudAllowedRef.current = false;
    setRouteNotice(null);
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
          {wakeOn && (
            <button
              className="wake-chip"
              title="Canela escucha «Hola, Canela» en este equipo. Clic para apagar."
              onClick={() => void voice.wakeSet(false).then(setWakeOn)}
            >
              👂 «Hola, Canela» · apagar
            </button>
          )}
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

      {routeAsk && (
        <div className="route-card" role="dialog" aria-label="Elegir modelo">
          <div className="route-title">
            {routeAsk.localPossible
              ? "Esta tarea parece compleja"
              : `Necesito ${routeAsk.offer.label} para esto`}
          </div>
          <p className="muted">
            {routeAsk.offer.why.charAt(0).toUpperCase() +
              routeAsk.offer.why.slice(1)}
            .{" "}
            {routeAsk.localPossible
              ? "Puedo intentarlo con el modelo local (gratis, no sale de tu equipo) o usar "
              : "Puedo usar "}
            {routeAsk.offer.label}: tu mensaje se enviará a su servicio en
            internet
            {routeAsk.offer.estCostUsd != null
              ? ` (≈ ${formatUsd(routeAsk.offer.estCostUsd)} estimado; el costo final lo fija el proveedor según los tokens reales)`
              : " (costo desconocido)"}
            .
          </p>
          {routeAsk.offer.ack.length > 0 && (
            <p className="route-warn">
              ⚠ Contiene {routeAsk.offer.ackSummary}. Sólo se enviará si eliges{" "}
              {routeAsk.offer.label}.
            </p>
          )}
          <div className="row">
            {routeAsk.localPossible && (
              <button className="btn" onClick={() => routeAsk.resolve("local")}>
                Seguir en local
              </button>
            )}
            <button
              className="btn primary"
              onClick={() => routeAsk.resolve("cloud")}
            >
              Usar {routeAsk.offer.label}
            </button>
            <button className="btn" onClick={() => routeAsk.resolve("cancel")}>
              Cancelar
            </button>
          </div>
          {routeAsk.offer.ack.length === 0 && (
            <button
              className="text-button"
              onClick={() => routeAsk.resolve("cloud-always")}
            >
              Usar {routeAsk.offer.label} cuando haga falta en esta conversación
            </button>
          )}
        </div>
      )}

      {privacyAsk && (
        <div className="route-card" role="dialog" aria-label="Autorizar envío">
          <div className="route-title">
            ¿Enviar esto a {providerLabel(privacyAsk.provider)}?
          </div>
          <p className="muted">
            Contiene {privacyAsk.summary}. Se enviará a su servicio en internet
            sólo si lo autorizas para este mensaje.
          </p>
          <div className="row">
            <button
              className="btn primary"
              onClick={() => privacyAsk.resolve(true)}
            >
              Enviar
            </button>
            <button className="btn" onClick={() => privacyAsk.resolve(false)}>
              No enviar
            </button>
          </div>
        </div>
      )}

      {routeNotice && (
        <div className="banner">
          {routeNotice}
          <button className="btn" onClick={() => setRouteNotice(null)}>
            Cerrar
          </button>
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
                  {m.agentName && m.agentName !== "Canela" && (
                    <span className="cost" title={m.agentReason}>
                      {" "}
                      · {m.agentName}
                    </span>
                  )}
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
                      {t.verified !== undefined && (
                        <span
                          className={`tool-check ${t.verified ? "" : "bad"}`}
                          title={t.verifyDetail}
                        >
                          {t.verified
                            ? " · comprobado"
                            : " · la comprobación falló"}
                        </span>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {m.plan && m.plan.length > 0 && (
                <PlanCard steps={m.plan} finished={!!m.planFinished} />
              )}
              {!m.plan && m.status !== "streaming" && (
                <ResultCard verdict={m.verdict} />
              )}
              {m.status !== "streaming" &&
                m.actions &&
                m.actions.length > 0 && (
                  <div className="action-summary">
                    {actionSummary(m.actions)}
                  </div>
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

/** v0.7.1: pasos del plan y, al terminar, qué se hizo, qué falta y errores. */
function PlanCard({
  steps,
  finished,
}: {
  steps: PlanStep[];
  finished: boolean;
}) {
  const r = finished ? planReport(steps) : null;
  return (
    <div className="plan-card">
      <ol className="plan-steps">
        {steps.map((s) => (
          <li key={s.index} className={s.status}>
            <span className="tool-icon">{PLAN_ICON[s.status]}</span>
            <strong>{s.name}</strong> — {s.task}
            {s.attempts > 1 && <span className="tool-why"> · 2 intentos</span>}
          </li>
        ))}
      </ol>
      {r && (
        <div className="plan-report">
          {(
            [
              ["Hecho", r.done],
              ["Pendiente", r.pending],
              ["Errores", r.errors],
              ["Sin verificar", r.unverified],
            ] as const
          ).map(
            ([title, items]) =>
              items.length > 0 && (
                <div key={title}>
                  <div className="plan-report-title">{title}</div>
                  <ul>
                    {items.map((t, i) => (
                      <li key={i}>{t}</li>
                    ))}
                  </ul>
                </div>
              ),
          )}
        </div>
      )}
    </div>
  );
}

/** v0.8: si se consiguió lo pedido, qué falló y cómo arreglarlo. */
function ResultCard({ verdict }: { verdict?: Verdict }) {
  const view = verdictView(verdict);
  if (!view) return null;
  return (
    <div className={`result-card ${verdict?.status ?? ""}`}>
      <div className="result-title">{view.title}</div>
      <ul>
        {view.lines.map((l, i) => (
          <li key={i}>
            <span className="tool-icon">{l.icon}</span>
            {l.text}
            {l.fix && (
              <div className="result-fix">Cómo arreglarlo: {l.fix}</div>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
