// Conversación por voz: micrófono → texto → respuesta hablada, con las
// reacciones de Canela. El panel coordina todo, incluso si está oculto.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  onTtsState,
  onVoiceState,
  onVoiceToggle,
  onVoiceTranscript,
  sendMood,
  voice,
  type Settings,
} from "../lib/ipc";
import { groupSentences, SentenceSplitter, spanishVoices } from "./speech";

export type VoicePhase = "idle" | "listening" | "processing" | "speaking";

export interface SpeechStream {
  feed(delta: string): void;
  end(): void;
}

export function useVoice(
  settings: Settings,
  onTranscript: (text: string) => void,
) {
  const [phase, setPhase] = useState<VoicePhase>("idle");
  const [level, setLevel] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const settingsRef = useRef(settings);
  const transcriptRef = useRef(onTranscript);
  const pendingLocal = useRef(0);
  const cloudPending = useRef(0);
  settingsRef.current = settings;
  transcriptRef.current = onTranscript;

  const watchdog = useRef<number>(undefined);
  const deadline = useRef(0);

  const speechDone = useCallback(() => {
    window.clearTimeout(watchdog.current);
    deadline.current = 0;
    setPhase((p) => (p === "speaking" ? "idle" : p));
    void sendMood({ mood: "happy" });
  }, []);

  /**
   * Seguro: si el motor de voz nunca avisa que terminó (p. ej. no hay voces
   * instaladas), libera el estado "hablando" al pasar el tiempo estimado.
   */
  const armWatchdog = useCallback(
    (ms: number) => {
      deadline.current = Math.max(deadline.current, Date.now() + ms);
      window.clearTimeout(watchdog.current);
      watchdog.current = window.setTimeout(() => {
        if (pendingLocal.current > 0 || cloudPending.current > 0) {
          pendingLocal.current = 0;
          cloudPending.current = 0;
          speechDone();
        }
      }, deadline.current - Date.now());
    },
    [speechDone],
  );

  const stopSpeaking = useCallback(() => {
    if (typeof speechSynthesis !== "undefined") speechSynthesis.cancel();
    pendingLocal.current = 0;
    void voice.cloudStop().catch(() => undefined);
    cloudPending.current = 0;
    window.clearTimeout(watchdog.current);
    deadline.current = 0;
    setPhase((p) => (p === "speaking" ? "idle" : p));
  }, []);

  /** Dice una o varias frases con el motor elegido. */
  const say = useCallback(
    (sentences: string[]) => {
      const s = settingsRef.current;
      if (!sentences.length) return;
      setPhase("speaking");
      void sendMood({ mood: "talk" });
      // Con Ollama no se envía nada a internet salvo permiso expreso.
      const cloudOk = s.provider !== "ollama" || s.ollamaCloudVoice;
      if (s.ttsEngine === "openai" && cloudOk) {
        for (const chunk of groupSentences(sentences)) {
          cloudPending.current += 1;
          armWatchdog(60_000);
          void voice.cloudSpeak(chunk).catch((e) => {
            cloudPending.current = Math.max(0, cloudPending.current - 1);
            setMessage(String(e));
          });
        }
        return;
      }
      if (typeof speechSynthesis === "undefined") {
        setMessage("Este equipo no tiene voces instaladas para hablar.");
        return;
      }
      const voices = spanishVoices(speechSynthesis.getVoices());
      const v = voices.find((x) => x.name === s.ttsVoice) ?? voices[0];
      for (const text of sentences) {
        const u = new SpeechSynthesisUtterance(text);
        if (v) u.voice = v;
        u.lang = v?.lang ?? "es-MX";
        u.rate = s.ttsRate;
        u.volume = s.ttsVolume;
        pendingLocal.current += 1;
        armWatchdog((text.length * 80) / s.ttsRate + 4000);
        u.onend = u.onerror = () => {
          pendingLocal.current = Math.max(0, pendingLocal.current - 1);
          if (pendingLocal.current === 0) speechDone();
        };
        speechSynthesis.speak(u);
      }
    },
    [speechDone, armWatchdog],
  );

  /** Prepara la lectura en voz alta de una respuesta que llega en partes. */
  const speechStream = useCallback(
    (spoken: boolean): SpeechStream | null => {
      const mode = settingsRef.current.speakReplies;
      if (mode === "never" || (mode === "voice" && !spoken)) return null;
      const splitter = new SentenceSplitter();
      return {
        feed: (d) => say(splitter.feed(d)),
        end: () => say(splitter.flush()),
      };
    },
    [say],
  );

  /** Botón de micrófono / atajo: escuchar, terminar o interrumpir. */
  const toggle = useCallback(async () => {
    setMessage(null);
    if (phase === "speaking") stopSpeaking();
    if (phase === "listening") {
      await voice.stop();
      return;
    }
    if (phase === "processing") return;
    try {
      await voice.start();
      setPhase("listening");
      void sendMood({ mood: "listen", bubble: "🎙 Te escucho…" });
    } catch (e) {
      setMessage(String(e));
      setPhase("idle");
      void sendMood({ mood: "sad", bubble: String(e) });
    }
  }, [phase, stopSpeaking]);

  const toggleRef = useRef(toggle);
  toggleRef.current = toggle;

  useEffect(() => {
    const un = [
      onVoiceState((e) => {
        if (e.state === "listening") {
          setPhase("listening");
          setLevel(e.level ?? 0);
        } else if (e.state === "processing") {
          setPhase("processing");
          setLevel(0);
          void sendMood({ mood: "curious" });
        } else {
          setPhase((p) => (p === "speaking" ? p : "idle"));
          setLevel(0);
          if (e.message) {
            setMessage(e.message);
            void sendMood(
              e.state === "error"
                ? { mood: "sad", bubble: e.message }
                : { mood: "idle", bubble: e.message },
            );
          } else if (e.state === "idle") {
            void sendMood({ mood: "idle" });
          }
        }
      }),
      onVoiceTranscript((text) => {
        setMessage(null);
        transcriptRef.current(text);
      }),
      onVoiceToggle(() => void toggleRef.current()),
      onTtsState((e) => {
        if (e.error) setMessage(e.error);
        // Cada fragmento avisa al terminar; Canela termina cuando no queda ninguno.
        if (!e.speaking && cloudPending.current > 0) {
          cloudPending.current -= 1;
          if (cloudPending.current === 0) speechDone();
        }
      }),
    ];
    return () => un.forEach((p) => void p.then((f) => f()));
  }, [speechDone]);

  return {
    phase,
    level,
    message,
    setMessage,
    toggle,
    stopSpeaking,
    speechStream,
    say,
  };
}
