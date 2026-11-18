// Envoltorios tipados de los comandos Rust y de los eventos entre ventanas.
import { Channel, invoke } from '@tauri-apps/api/core';
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event';

export type ProviderId = 'anthropic' | 'openai';

export interface Settings {
  provider: ProviderId;
  anthropicModel: string;
  anthropicEffort: 'low' | 'medium' | 'high' | 'xhigh' | 'max';
  openaiModel: string;
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
  ttsEngine: 'local' | 'openai';
  ttsVoice: string;
  ttsCloudModel: string;
  ttsCloudVoice: string;
  ttsRate: number;
  ttsVolume: number;
  speakReplies: 'voice' | 'always' | 'never';
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
  role: 'user' | 'assistant';
  text: string;
  provider?: ProviderId;
  transcript?: unknown[];
  images?: ImageData[];
}

export type StreamEvent =
  | { type: 'started'; model: string }
  | { type: 'thinking' }
  | { type: 'delta'; text: string }
  | { type: 'toolStart'; callId: string; name: string; label: string }
  | { type: 'toolEnd'; callId: string; ok: boolean; summary: string }
  | { type: 'confirmRequest'; confirmId: string; title: string; details: string; diff: string | null; image: string | null }
  | {
      type: 'done';
      stopReason: string | null;
      model: string | null;
      notice: string | null;
      provider: ProviderId;
      transcript: unknown[];
      usage: Usage;
      costUsd: number | null;
    }
  | { type: 'error'; message: string }
  | { type: 'cancelled' };

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
  source: 'api' | 'tabla';
  note: string | null;
}

export interface JobInfo {
  id: string;
  title: string;
  status: 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
  progress: string | null;
  result: string | null;
  error: string | null;
  createdAt: string;
}

export const api = {
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<Settings>('save_settings', { settings }),
  keyStatus: () => invoke<KeyStatus>('api_key_status'),
  setApiKey: (provider: ProviderId, key: string) => invoke<void>('set_api_key', { provider, key }),
  deleteApiKey: (provider: ProviderId) => invoke<void>('delete_api_key', { provider }),
  showPanel: () => invoke<void>('show_panel'),
  hidePanel: () => invoke<void>('hide_panel'),
  togglePet: () => invoke<void>('toggle_pet'),
  quit: () => invoke<void>('quit_app'),
  confirmAction: (confirmId: string, approved: boolean) => invoke<boolean>('confirm_action', { confirmId, approved }),
  jobsList: () => invoke<JobInfo[]>('jobs_list'),
  jobCancel: (id: string) => invoke<boolean>('job_cancel', { id }),
  allowedFolders: () => invoke<string[]>('allowed_folders'),
  modelInfo: () => invoke<ModelInfo>('model_info'),
  summarize: (messages: WireMessage[], userName: string) => invoke<string>('summarize_conversation', { messages, userName }),
  prepareAttachment: (path: string) =>
    invoke<{ name: string; kind: 'text' | 'image'; text: string | null; image: ImageData | null; note: string | null }>('prepare_attachment', { path }),

  /** Inicia una respuesta en streaming. Devuelve una función para cancelarla. */
  async chat(
    requestId: string,
    messages: WireMessage[],
    userName: string,
    context: string,
    onEvent: (ev: StreamEvent) => void,
  ): Promise<() => Promise<void>> {
    const channel = new Channel<StreamEvent>();
    channel.onmessage = onEvent;
    await invoke('chat_send', { requestId, messages, userName, context, onEvent: channel });
    return async () => {
      await invoke('chat_cancel', { requestId });
    };
  },
};

// ------------------------- Eventos entre ventanas ---------------------------

export type Mood = 'idle' | 'listen' | 'think' | 'curious' | 'talk' | 'happy' | 'sad' | 'celebrate';

export interface MoodPayload {
  mood: Mood;
  /** Texto opcional para la burbuja de Canela. */
  bubble?: string;
}

export const sendMood = (payload: MoodPayload) => emitTo('pet', 'canela-mood', payload);

export const onMood = (cb: (p: MoodPayload) => void): Promise<UnlistenFn> =>
  listen<MoodPayload>('canela-mood', (e) => cb(e.payload));

export const onSettingsChanged = (cb: (s: Settings) => void): Promise<UnlistenFn> =>
  listen<Settings>('settings-changed', (e) => cb(e.payload));

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

export type TaskFilter = 'today' | 'pending' | 'done' | 'all';

export const MEMORY_KINDS: Array<{ id: string; label: string }> = [
  { id: 'proyecto', label: 'Proyecto' },
  { id: 'preferencia', label: 'Preferencia' },
  { id: 'horario', label: 'Horario' },
  { id: 'aplicacion', label: 'Aplicación' },
  { id: 'persona', label: 'Persona' },
  { id: 'dato', label: 'Dato importante' },
  { id: 'otro', label: 'Otro' },
];

export const data = {
  memoryList: (kind?: string, query?: string) => invoke<Memory[]>('memory_list', { kind: kind || null, query: query || null }),
  memoryAdd: (kind: string, content: string) => invoke<Memory>('memory_add', { kind, content }),
  memoryUpdate: (id: number, kind: string, content: string) => invoke<Memory>('memory_update', { id, kind, content }),
  memoryDelete: (id: number) => invoke<boolean>('memory_delete', { id }),
  memoryClear: () => invoke<number>('memory_clear'),
  memorySetTemporary: (id: number, days: number | null) => invoke<Memory>('memory_set_temporary', { id, days }),
  taskList: (filter: TaskFilter) => invoke<Task[]>('task_list', { filter }),
  taskAdd: (input: TaskInput) => invoke<Task>('task_add', { input }),
  taskUpdate: (id: number, input: TaskInput) => invoke<Task>('task_update', { id, input }),
  taskSetDone: (id: number, done: boolean) => invoke<Task>('task_set_done', { id, done }),
  taskDelete: (id: number) => invoke<boolean>('task_delete', { id }),
  activityList: () => invoke<ActivityItem[]>('activity_list'),
  conversationSave: (id: string, title: string, payload: string) => invoke<void>('conversation_save', { id, title, data: payload }),
  conversationList: () => invoke<ConversationMeta[]>('conversation_list'),
  conversationGet: (id: string) => invoke<string | null>('conversation_get', { id }),
  conversationDelete: (id: string) => invoke<boolean>('conversation_delete', { id }),
  conversationClear: () => invoke<number>('conversation_clear'),
  conversationContext: () => invoke<string>('conversation_context'),
  docIndexStats: () => invoke<{ files: number; updatedAt: string | null }>('doc_index_stats'),
  docIndexClear: () => invoke<void>('doc_index_clear'),
  routineList: () => invoke<Routine[]>('routine_list'),
  routineDelete: (name: string) => invoke<void>('routine_delete', { name }),
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
    return Array.isArray(v) ? (v as RoutineStep[]).filter((x) => x && typeof x.herramienta === 'string') : [];
  } catch {
    return [];
  }
}

export const onDataChanged = (cb: (kind: string) => void): Promise<UnlistenFn> =>
  listen<{ kind: string }>('data-changed', (e) => cb(e.payload.kind));

export const onJobsChanged = (cb: (jobs: JobInfo[]) => void): Promise<UnlistenFn> =>
  listen<JobInfo[]>('jobs-changed', (e) => cb(e.payload));

export const onJobFinished = (cb: (job: JobInfo) => void): Promise<UnlistenFn> =>
  listen<JobInfo>('job-finished', (e) => cb(e.payload));

// ------------------------------------------------------------------ Voz

export interface VoiceStateEvent {
  state: 'listening' | 'processing' | 'idle' | 'error';
  level: number | null;
  message: string | null;
}

export const voice = {
  devices: () => invoke<{ inputs: string[]; outputs: string[] }>('voice_devices'),
  start: () => invoke<void>('voice_start'),
  stop: () => invoke<boolean>('voice_stop'),
  cancel: () => invoke<boolean>('voice_cancel'),
  cloudSpeak: (text: string) => invoke<void>('tts_cloud_speak', { text }),
  cloudStop: () => invoke<void>('tts_cloud_stop'),
};

export const onVoiceState = (cb: (e: VoiceStateEvent) => void): Promise<UnlistenFn> =>
  listen<VoiceStateEvent>('voice-state', (e) => cb(e.payload));

export const onVoiceTranscript = (cb: (text: string) => void): Promise<UnlistenFn> =>
  listen<{ text: string }>('voice-transcript', (e) => cb(e.payload.text));

export const onVoiceToggle = (cb: () => void): Promise<UnlistenFn> => listen('voice-toggle', () => cb());

export const onTtsState = (cb: (e: { speaking: boolean; error?: string }) => void): Promise<UnlistenFn> =>
  listen<{ speaking: boolean; error?: string }>('tts-state', (e) => cb(e.payload));

// ---------------------------------------------------------------- Pantalla

export interface ScreenSources {
  monitors: Array<{ id: number; name: string; width: number; height: number; primary: boolean }>;
  windows: Array<{ id: number; title: string; app: string }>;
}

export type CaptureTarget = { kind: 'monitor'; id: number } | { kind: 'window'; id: number } | { kind: 'active' };

export const screen = {
  sources: () => invoke<ScreenSources>('screen_sources'),
  capture: (target: CaptureTarget) =>
    invoke<{ captureId: string; preview: string; width: number; height: number }>('screen_capture', { target }),
  finalize: (captureId: string, crop: { x: number; y: number; w: number; h: number } | null) =>
    invoke<ImageData>('screen_finalize', { captureId, crop }),
  discard: (captureId: string) => invoke<void>('screen_discard', { captureId }),
};

export const imageUrl = (i: ImageData) => `data:${i.mediaType};base64,${i.data}`;

/** "≈ 0,0042 US$" */
export function formatUsd(v: number): string {
  if (v > 0 && v < 0.0001) return '< 0,0001 US$';
  return `${v.toLocaleString('es', { minimumFractionDigits: v < 1 ? 4 : 2, maximumFractionDigits: v < 1 ? 4 : 2 })} US$`;
}
