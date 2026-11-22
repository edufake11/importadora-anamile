// Envoltorios tipados de los comandos Rust y de los eventos entre ventanas.
import { Channel, invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ProviderId = "anthropic" | "openai" | "ollama";

export const PROVIDER_NAMES: Record<ProviderId, string> = {
  anthropic: "Claude",
  openai: "ChatGPT",
  ollama: "Ollama (local)",
};

/** Ollama funciona sin clave API. */
export const needsKey = (p: ProviderId) => p !== "ollama";

/** ¿Hay clave (o no hace falta) para chatear con este proveedor? */
export const canChat = (
  p: ProviderId,
  keys: { anthropic: boolean; openai: boolean },
) => (p === "ollama" ? true : keys[p]);

export interface Settings {
  provider: ProviderId;
  anthropicModel: string;
  anthropicEffort: "low" | "medium" | "high" | "xhigh" | "max";
  openaiModel: string;
  ollamaUrl: string;
  ollamaModel: string;
  ollamaNumCtx: number;
  ollamaThink: boolean;
  ollamaCloudVoice: boolean;
  resourceMode: "balanced" | "saver";
  routerAuto: boolean;
  cloudPreference: ProviderId;
  monthlyBudgetUsd: number;
  memoryLog: boolean;
  userName: string;
  petScale: number;
  wander: boolean;
  replyBubbles: boolean;
  reduceMotion: boolean;
  hideInFullscreen: boolean;
  greetOnStartup: boolean;
  launchAtStartup: boolean;
  saveConversations: boolean;
  autoOpenApps: boolean;
  autoOpenWeb: boolean;
  autoMediaControl: boolean;
  allowedFolders: string[] | null;
  voiceConsent: boolean;
  voiceInputDevice: string | null;
  voiceOutputDevice: string | null;
  sttModel: string;
  ttsEngine: "local" | "openai" | "piper";
  sttEngine: "local" | "openai";
  sttLocalModel: string;
  piperVoice: string;
  ttsVoice: string;
  ttsCloudModel: string;
  ttsCloudVoice: string;
  ttsRate: number;
  ttsVolume: number;
  speakReplies: "voice" | "always" | "never";
  wakeWord: boolean;
  visionEnabled: boolean;
  confirmFileSharing: boolean;
}

export interface ImageData {
  mediaType: string;
  data: string;
  width: number;
  height: number;
}

export interface KeyStatus {
  anthropic: boolean;
  openai: boolean;
}

/** Mensaje tal como se envía a Rust. */
export interface WireMessage {
  role: "user" | "assistant";
  text: string;
  provider?: ProviderId;
  transcript?: unknown[];
  images?: ImageData[];
  /** v0.7: agente que dio esta respuesta (para la continuidad). */
  agent?: AgentId;
}

export type AgentId =
  | "general"
  | "programador"
  | "investigador"
  | "productividad"
  | "archivos"
  | "sistema";

/** v0.7: acción ejecutada por un agente, tal como la registró el backend. */
export interface ActionRecord {
  tool: string;
  label: string;
  /** "ok" | "error" | "denegada" | "no permitida" */
  status: string;
  /** true = comprobado; false = la comprobación falló; null = no comprobable. */
  verified: boolean | null;
  detail: string | null;
  /** v0.8: avance medido [hecho, total] (rutinas, organizar…). */
  progress?: [number, number] | null;
}

/** v0.8: un objetivo de la petición y si se cumplió (lo comprueba el programa). */
export interface VerdictItem {
  what: string;
  status:
    | "cumplido"
    | "sin_verificar"
    | "parcial"
    | "fallido"
    | "no_intentado"
    | "no_verificable";
  evidence: string | null;
  problem: string | null;
  fix: string | null;
}

/** v0.8: ¿se consiguió lo pedido? */
export interface Verdict {
  status:
    | "completo"
    | "parcial"
    | "no_logrado"
    | "no_verificable"
    | "sin_comprobar";
  items: VerdictItem[];
  summary: string;
}

/** v0.7.1: un paso de un plan coordinado entre agentes. */
export interface PlanStep {
  index: number;
  agent: AgentId;
  name: string;
  task: string;
  status:
    | "pendiente"
    | "en_curso"
    | "hecho"
    | "sin_verificar"
    | "incompleto"
    | "no_logrado"
    | "error"
    | "cancelado";
  attempts: number;
  actions: ActionRecord[];
  warnings: string[];
  error: string | null;
  verdict?: Verdict | null;
}

export type StreamEvent =
  | { type: "started"; model: string }
  | { type: "thinking" }
  | { type: "delta"; text: string }
  | { type: "toolStart"; callId: string; name: string; label: string }
  | { type: "toolEnd"; callId: string; ok: boolean; summary: string }
  | { type: "agent"; id: AgentId; name: string; reason: string }
  | { type: "toolVerified"; callId: string; ok: boolean; detail: string }
  | { type: "plan"; steps: PlanStep[]; finished: boolean }
  | {
      type: "report";
      agent: string;
      actions: ActionRecord[];
      warnings: string[];
      verdict?: Verdict | null;
    }
  | {
      type: "confirmRequest";
      confirmId: string;
      title: string;
      details: string;
      diff: string | null;
      image: string | null;
    }
  | {
      type: "done";
      stopReason: string | null;
      model: string | null;
      notice: string | null;
      provider: ProviderId;
      transcript: unknown[];
      usage: Usage;
      costUsd: number | null;
    }
  | { type: "error"; message: string }
  | { type: "cancelled" };

export interface Usage {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}

export interface ModelInfo {
  provider: string;
  model: string;
  exists: boolean | null;
  supportsImages: boolean | null;
  supportsThinking: boolean | null;
  supportsEffort: boolean | null;
  contextTokens: number | null;
  maxOutput: number | null;
  price: { input: number; output: number; cacheRead: number } | null;
  source: "api" | "tabla" | "ollama";
  note: string | null;
}

export interface OllamaStatus {
  url: string;
  running: boolean;
  version: string | null;
  models: { name: string; sizeGb: number; parameters: string | null }[];
  model: string;
  modelInstalled: boolean;
  capabilities: string[];
  contextLength: number | null;
  message: string;
}

export interface JobInfo {
  id: string;
  title: string;
  status: "queued" | "running" | "done" | "failed" | "cancelled";
  progress: string | null;
  result: string | null;
  error: string | null;
  createdAt: string;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<Settings>("save_settings", { settings }),
  keyStatus: () => invoke<KeyStatus>("api_key_status"),
  setApiKey: (provider: ProviderId, key: string) =>
    invoke<void>("set_api_key", { provider, key }),
  deleteApiKey: (provider: ProviderId) =>
    invoke<void>("delete_api_key", { provider }),
  showPanel: () => invoke<void>("show_panel"),
  hidePanel: () => invoke<void>("hide_panel"),
  togglePet: () => invoke<void>("toggle_pet"),
  quit: () => invoke<void>("quit_app"),
  confirmAction: (confirmId: string, approved: boolean) =>
    invoke<boolean>("confirm_action", { confirmId, approved }),
  jobsList: () => invoke<JobInfo[]>("jobs_list"),
  jobCancel: (id: string) => invoke<boolean>("job_cancel", { id }),
  allowedFolders: () => invoke<string[]>("allowed_folders"),
  modelInfo: () => invoke<ModelInfo>("model_info"),
  memoryStatus: () => invoke<MemStatus>("memory_status"),
  memoryMeasureNow: () => invoke<MemStatus>("memory_measure_now"),
  memoryRelease: () =>
    invoke<{ pending: boolean; message: string }>("memory_release"),
  memoryExport: (path: string) => invoke<string>("memory_export", { path }),
  aiPreload: () => invoke<boolean>("ai_preload"),
  /** Qué modelo usar para este mensaje (no envía nada a ninguna IA). */
  routeDecide: (request: RouteRequest) =>
    invoke<RouteDecision>("route_decide", { request }),
  spendSummary: () => invoke<SpendSummary>("spend_summary"),
  ollamaStatus: (model?: string) =>
    invoke<OllamaStatus>("ollama_status", { model: model ?? null }),
  summarize: (messages: WireMessage[], userName: string) =>
    invoke<string>("summarize_conversation", { messages, userName }),
  prepareAttachment: (path: string) =>
    invoke<{
      name: string;
      kind: "text" | "image";
      text: string | null;
      image: ImageData | null;
      note: string | null;
    }>("prepare_attachment", { path }),

  /** Inicia una respuesta en streaming. Devuelve una función para cancelarla. */
  async chat(
    requestId: string,
    messages: WireMessage[],
    userName: string,
    context: string,
    onEvent: (ev: StreamEvent) => void,
    providerOverride?: ProviderId,
    privacyAck?: string[],
  ): Promise<() => Promise<void>> {
    const channel = new Channel<StreamEvent>();
    channel.onmessage = onEvent;
    await invoke("chat_send", {
      requestId,
      messages,
      userName,
      context,
      providerOverride: providerOverride ?? null,
      privacyAck: privacyAck ?? null,
      onEvent: channel,
    });
    return async () => {
      await invoke("chat_cancel", { requestId });
    };
  },
};

// ------------------------- Eventos entre ventanas ---------------------------

export type Mood =
  | "idle"
  | "listen"
  | "think"
  | "curious"
  | "talk"
  | "happy"
  | "sad"
  | "celebrate";

export interface MoodPayload {
  mood: Mood;
  /** Texto opcional para la burbuja de Canela. */
  bubble?: string;
}

export const sendMood = (payload: MoodPayload) =>
  emitTo("pet", "canela-mood", payload);

export const onMood = (cb: (p: MoodPayload) => void): Promise<UnlistenFn> =>
  listen<MoodPayload>("canela-mood", (e) => cb(e.payload));

export const onSettingsChanged = (
  cb: (s: Settings) => void,
): Promise<UnlistenFn> =>
  listen<Settings>("settings-changed", (e) => cb(e.payload));

// ------------------------------ Datos locales --------------------------------

export interface Memory {
  id: number;
  kind: string;
  content: string;
  source: string;
  createdAt: string;
  updatedAt: string;
  /** Recuerdo temporal: se olvida solo después de esta fecha. */
  expiresAt: string | null;
}

export interface Task {
  id: number;
  title: string;
  notes: string;
  priority: number;
  due: string | null;
  remindAt: string | null;
  reminded: boolean;
  done: boolean;
  createdAt: string;
  completedAt: string | null;
}

export interface TaskInput {
  title?: string;
  notes?: string;
  priority?: number;
  /** '' quita la fecha */
  due?: string;
  /** '' quita el recordatorio */
  remindAt?: string;
}

export interface ActivityItem {
  id: number;
  at: string;
  kind: string;
  summary: string;
}

export interface ConversationMeta {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
}

export type TaskFilter = "today" | "pending" | "done" | "all";

export const MEMORY_KINDS: Array<{ id: string; label: string }> = [
  { id: "proyecto", label: "Proyecto" },
  { id: "preferencia", label: "Preferencia" },
  { id: "horario", label: "Horario" },
  { id: "aplicacion", label: "Aplicación" },
  { id: "persona", label: "Persona" },
  { id: "dato", label: "Dato importante" },
  { id: "otro", label: "Otro" },
];

export const data = {
  memoryList: (kind?: string, query?: string) =>
    invoke<Memory[]>("memory_list", {
      kind: kind || null,
      query: query || null,
    }),
  memoryAdd: (kind: string, content: string) =>
    invoke<Memory>("memory_add", { kind, content }),
  memoryUpdate: (id: number, kind: string, content: string) =>
    invoke<Memory>("memory_update", { id, kind, content }),
  memoryDelete: (id: number) => invoke<boolean>("memory_delete", { id }),
  memoryClear: () => invoke<number>("memory_clear"),
  memorySetTemporary: (id: number, days: number | null) =>
    invoke<Memory>("memory_set_temporary", { id, days }),
  taskList: (filter: TaskFilter) => invoke<Task[]>("task_list", { filter }),
  taskAdd: (input: TaskInput) => invoke<Task>("task_add", { input }),
  taskUpdate: (id: number, input: TaskInput) =>
    invoke<Task>("task_update", { id, input }),
  taskSetDone: (id: number, done: boolean) =>
    invoke<Task>("task_set_done", { id, done }),
  taskDelete: (id: number) => invoke<boolean>("task_delete", { id }),
  activityList: () => invoke<ActivityItem[]>("activity_list"),
  conversationSave: (id: string, title: string, payload: string) =>
    invoke<void>("conversation_save", { id, title, data: payload }),
  conversationList: () => invoke<ConversationMeta[]>("conversation_list"),
  conversationGet: (id: string) =>
    invoke<string | null>("conversation_get", { id }),
  conversationDelete: (id: string) =>
    invoke<boolean>("conversation_delete", { id }),
  conversationClear: () => invoke<number>("conversation_clear"),
  conversationContext: () => invoke<string>("conversation_context"),
  docIndexStats: () =>
    invoke<{ files: number; updatedAt: string | null }>("doc_index_stats"),
  docIndexClear: () => invoke<void>("doc_index_clear"),
  routineList: () => invoke<Routine[]>("routine_list"),
  routineDelete: (name: string) => invoke<void>("routine_delete", { name }),
};

export interface Routine {
  name: string;
  /** JSON: [{herramienta, parametros, descripcion}] */
  steps: string;
  updatedAt: string;
}

export interface RoutineStep {
  herramienta: string;
  parametros?: Record<string, unknown>;
  descripcion?: string;
}

/** Lee los pasos guardados sin fallar si el JSON está dañado. */
export function routineSteps(r: Routine): RoutineStep[] {
  try {
    const v: unknown = JSON.parse(r.steps);
    return Array.isArray(v)
      ? (v as RoutineStep[]).filter(
          (x) => x && typeof x.herramienta === "string",
        )
      : [];
  } catch {
    return [];
  }
}

export const onDataChanged = (
  cb: (kind: string) => void,
): Promise<UnlistenFn> =>
  listen<{ kind: string }>("data-changed", (e) => cb(e.payload.kind));

export interface MemProcess {
  pid: number;
  name: string;
  path: string | null;
  private: number;
  workingSet: number;
}

export interface MemSample {
  ts: string;
  sys: {
    total: number;
    available: number;
    commitUsed: number;
    commitLimit: number;
    commitPeak: number | null;
    cache: number | null;
  };
  gpu: { name: string; used: number; total: number } | null;
  ai:
    | {
        name: string;
        size: number;
        sizeVram: number;
        contextLength: number | null;
        expiresAt: string | null;
      }[]
    | null;
  canelaPrivate: number;
  canelaWorkingSet: number;
  top: MemProcess[];
}

export interface MemEvent {
  ts: string;
  kind: string;
  message: string;
  pid: number | null;
  name: string | null;
  bytes: number | null;
}

export interface MemStatus {
  mode: string;
  latest: MemSample | null;
  events: MemEvent[];
  history: {
    ts: string;
    available: number;
    commitUsed: number;
    vramUsed: number | null;
    aiLoaded: boolean;
    canela: number;
  }[];
  busy: boolean;
  lowRam: boolean;
  idleUnloadSecs: number;
  logDir: string;
  logging: boolean;
}

/** Bytes → «7,5 GB». */
export const fmtGb = (b: number) =>
  `${(b / 1024 ** 3).toLocaleString("es", { minimumFractionDigits: 1, maximumFractionDigits: 1 })} GB`;

export const onMemoryWarning = (
  cb: (e: MemEvent | null) => void,
): Promise<UnlistenFn> =>
  Promise.all([
    listen<MemEvent>("memory-warning", (e) => cb(e.payload)),
    listen("memory-warning-clear", () => cb(null)),
  ]).then((fs) => () => fs.forEach((f) => f()));

export const onJobsChanged = (
  cb: (jobs: JobInfo[]) => void,
): Promise<UnlistenFn> =>
  listen<JobInfo[]>("jobs-changed", (e) => cb(e.payload));

export const onJobFinished = (
  cb: (job: JobInfo) => void,
): Promise<UnlistenFn> => listen<JobInfo>("job-finished", (e) => cb(e.payload));

// ------------------------------------------------------------------ Voz

export interface VoiceStateEvent {
  state: "listening" | "processing" | "idle" | "error";
  level: number | null;
  message: string | null;
}

export const voice = {
  devices: () =>
    invoke<{ inputs: string[]; outputs: string[] }>("voice_devices"),
  start: () => invoke<void>("voice_start"),
  stop: () => invoke<boolean>("voice_stop"),
  cancel: () => invoke<boolean>("voice_cancel"),
  cloudSpeak: (text: string) => invoke<void>("tts_cloud_speak", { text }),
  cloudStop: () => invoke<void>("tts_cloud_stop"),
  /** Piper: voz natural sin internet (usa la misma cola y se para con cloudStop). */
  localSpeak: (text: string) => invoke<void>("tts_local_speak", { text }),
  localStatus: () => invoke<LocalVoiceStatus>("local_voice_status"),
  localDownload: (id: string) => invoke<void>("local_voice_download", { id }),
  localCancel: () => invoke<void>("local_voice_cancel"),
  localDelete: (id: string) => invoke<void>("local_voice_delete", { id }),
  /** «Hola, Canela» (escucha local). */
  wakeSet: (enabled: boolean) => invoke<boolean>("wake_set", { enabled }),
  wakeStatus: () => invoke<boolean>("wake_status"),
  wakePause: (paused: boolean) => invoke<void>("wake_pause", { paused }),
};

export const onWakeState = (
  cb: (e: { active: boolean; error?: string | null }) => void,
): Promise<UnlistenFn> =>
  listen<{ active: boolean; error?: string | null }>("wake-state", (e) =>
    cb(e.payload),
  );

export interface LocalAsset {
  id: string;
  kind: "stt" | "voice" | "engine";
  label: string;
  note: string;
  license: string;
  size: number;
  installed: boolean;
  supported: boolean;
}

export interface LocalVoiceStatus {
  sttAvailable: boolean;
  assets: LocalAsset[];
  loadedStt: string | null;
  downloading: string | null;
  dir: string;
}

export interface DownloadEvent {
  id: string;
  done?: number;
  total?: number;
  finished?: boolean;
  error?: string | null;
}

export const onVoiceDownload = (
  cb: (e: DownloadEvent) => void,
): Promise<UnlistenFn> =>
  listen<DownloadEvent>("voice-download", (e) => cb(e.payload));

/** 190085487 → «190 MB». */
export const fmtMb = (b: number) =>
  b >= 1e9
    ? `${(b / 1e9).toLocaleString("es", { maximumFractionDigits: 1 })} GB`
    : `${Math.round(b / 1e6)} MB`;

export const onVoiceState = (
  cb: (e: VoiceStateEvent) => void,
): Promise<UnlistenFn> =>
  listen<VoiceStateEvent>("voice-state", (e) => cb(e.payload));

export const onVoiceTranscript = (
  cb: (text: string) => void,
): Promise<UnlistenFn> =>
  listen<{ text: string }>("voice-transcript", (e) => cb(e.payload.text));

export const onVoiceToggle = (cb: () => void): Promise<UnlistenFn> =>
  listen("voice-toggle", () => cb());

export const onTtsState = (
  cb: (e: { speaking: boolean; error?: string }) => void,
): Promise<UnlistenFn> =>
  listen<{ speaking: boolean; error?: string }>("tts-state", (e) =>
    cb(e.payload),
  );

// ---------------------------------------------------------------- Pantalla

export interface ScreenSources {
  monitors: Array<{
    id: number;
    name: string;
    width: number;
    height: number;
    primary: boolean;
  }>;
  windows: Array<{ id: number; title: string; app: string }>;
}

export type CaptureTarget =
  | { kind: "monitor"; id: number }
  | { kind: "window"; id: number }
  | { kind: "active" };

export const screen = {
  sources: () => invoke<ScreenSources>("screen_sources"),
  capture: (target: CaptureTarget) =>
    invoke<{
      captureId: string;
      preview: string;
      width: number;
      height: number;
    }>("screen_capture", { target }),
  finalize: (
    captureId: string,
    crop: { x: number; y: number; w: number; h: number } | null,
  ) => invoke<ImageData>("screen_finalize", { captureId, crop }),
  discard: (captureId: string) => invoke<void>("screen_discard", { captureId }),
};

export const imageUrl = (i: ImageData) =>
  `data:${i.mediaType};base64,${i.data}`;

/** "≈ 0,0042 US$" */
export function formatUsd(v: number): string {
  if (v > 0 && v < 0.0001) return "< 0,0001 US$";
  return `${v.toLocaleString("es", { minimumFractionDigits: v < 1 ? 4 : 2, maximumFractionDigits: v < 1 ? 4 : 2 })} US$`;
}

/**
 * Pregunta «Sí / No» con el diálogo nativo del sistema. Si el diálogo no se
 * puede mostrar, responde NO (nunca se borra ni se descarga sin permiso).
 */
export async function askUser(message: string): Promise<boolean> {
  try {
    return await ask(message, {
      title: "Canela",
      kind: "warning",
      okLabel: "Sí",
      cancelLabel: "No",
    });
  } catch {
    return false;
  }
}

// ------------------------- Enrutador de modelos (v0.6.1) --------------------

export interface RouteRequest {
  text: string;
  hasImages: boolean;
  attachmentChars: number;
  historyChars: number;
  /** Contenido de adjuntos, textos anteriores e imágenes previas: se revisan
   * por privacidad antes de enviar nada fuera. */
  attachments: string[];
  history: string[];
  historyImages: number;
}

export interface PrivacyFinding {
  level: "none" | "uncertain" | "personal" | "secret";
  kind: string;
  what: string;
  place: string;
}

export interface CloudOffer {
  provider: ProviderId;
  model: string;
  label: string;
  /** Costo ESTIMADO (el real depende de los tokens que informe el proveedor). */
  estCostUsd: number | null;
  estSeconds: number;
  why: string;
  /** Datos que necesitan tu autorización explícita para salir del equipo. */
  ack: string[];
  ackSummary: string;
}

export interface RouteCandidate {
  provider: ProviderId;
  model: string;
  label: string;
  eligible: boolean;
  whyNot: string | null;
  quality: number;
  estCostUsd: number | null;
  estSeconds: number;
}

export interface RouteDecision {
  provider: ProviderId;
  complexity: "simple" | "medium" | "complex";
  reasons: string[];
  offer: CloudOffer | null;
  blocked: string | null;
  private: boolean;
  privacy: { findings: PrivacyFinding[] };
  ackRequired: string[];
  candidates: RouteCandidate[];
  spentMonthUsd: number;
  reservedUsd: number;
  budgetUsd: number;
}

export interface SpendSummary {
  month: string;
  spentUsd: number;
  estimatedUsd: number;
  budgetUsd: number;
  breakdown: { provider: string; model: string; calls: number; usd: number }[];
}

/** Qué hacer con la decisión del enrutador antes de enviar. */
export type RouteAction =
  | { kind: "send"; override?: ProviderId; ack?: string[] }
  | { kind: "ask"; offer: CloudOffer; localPossible: boolean }
  | {
      kind: "confirm-send";
      provider: ProviderId;
      ack: string[];
      summary: string;
    }
  | { kind: "blocked"; message: string };

const PROVIDER_LABEL: Record<ProviderId, string> = {
  anthropic: "Claude",
  openai: "ChatGPT",
  ollama: "el modelo local",
};

export function routeAction(
  d: RouteDecision,
  cloudAllowedInConversation: boolean,
  main: ProviderId,
): RouteAction {
  if (d.blocked && !d.offer) return { kind: "blocked", message: d.blocked };
  // Principal externo: datos personales → autorización explícita para ESTE mensaje.
  if (main !== "ollama") {
    if (d.provider !== main) return { kind: "send", override: d.provider };
    if (d.ackRequired.length)
      return {
        kind: "confirm-send",
        provider: main,
        ack: d.ackRequired,
        summary: d.privacy.findings
          .filter((f) => f.level === "personal" || f.level === "uncertain")
          .map((f) => `${f.what} (${f.place})`)
          .join(", "),
      };
    return { kind: "send" };
  }
  if (d.offer) {
    const localPossible = !d.blocked && d.provider === "ollama";
    // «Permitir en esta conversación» NO cubre datos personales ni imágenes.
    if (cloudAllowedInConversation && !d.private && d.offer.ack.length === 0)
      return { kind: "send", override: d.offer.provider };
    return { kind: "ask", offer: d.offer, localPossible };
  }
  return { kind: "send" };
}

export const providerLabel = (p: ProviderId) => PROVIDER_LABEL[p];
