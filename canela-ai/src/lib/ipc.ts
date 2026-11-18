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
  allowedFolders: string[] | null;
}

export interface KeyStatus {
  anthropic: boolean;
  openai: boolean;
}

/** Mensaje tal como se envía a Rust. */
export interface WireMessage {
  role: 'user' | 'assistant';
  text: string;
  anthropicContent?: unknown;
}

export type StreamEvent =
  | { type: 'started'; model: string }
  | { type: 'thinking' }
  | { type: 'delta'; text: string }
  | { type: 'done'; stopReason: string | null; model: string | null; anthropicContent: unknown | null; notice: string | null }
  | { type: 'error'; message: string }
  | { type: 'cancelled' };

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

  /** Inicia una respuesta en streaming. Devuelve una función para cancelarla. */
  async chat(
    requestId: string,
    messages: WireMessage[],
    userName: string,
    onEvent: (ev: StreamEvent) => void,
  ): Promise<() => Promise<void>> {
    const channel = new Channel<StreamEvent>();
    channel.onmessage = onEvent;
    await invoke('chat_send', { requestId, messages, userName, onEvent: channel });
    return async () => {
      await invoke('chat_cancel', { requestId });
    };
  },
};

// ------------------------- Eventos entre ventanas ---------------------------

export type Mood = 'idle' | 'listen' | 'think' | 'talk' | 'happy' | 'sad' | 'celebrate';

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
