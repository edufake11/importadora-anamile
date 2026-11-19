// Utilidades de voz hablada (sin dependencias de Tauri, para poder probarlas).

const CODE_NOTE = "Te dejé el código en el chat.";

/** Quita lo que no se debe leer en voz alta (markdown, enlaces, emojis). */
export function cleanForSpeech(text: string): string {
  return text
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/https?:\/\/\S+/g, "un enlace")
    .replace(/(\*\*|__|\*|~~)/g, "")
    .replace(/^\s{0,3}(#{1,6}|[-*+]|\d+\.)\s+/gm, "")
    .replace(/\p{Extended_Pictographic}/gu, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Divide el texto que llega en streaming en frases listas para decir.
 * Los bloques de código no se leen: se anuncia una sola vez que están en el chat.
 */
export class SentenceSplitter {
  private buf = "";
  private pos = 0;
  private inCode = false;

  feed(delta: string): string[] {
    this.buf += delta;
    return this.drain(false);
  }

  flush(): string[] {
    return this.drain(true);
  }

  private drain(final: boolean): string[] {
    const out: string[] = [];
    const push = (t: string) => {
      const c = cleanForSpeech(t);
      if (c.length > 1) out.push(c);
    };
    for (;;) {
      const rest = this.buf.slice(this.pos);
      if (this.inCode) {
        const end = rest.indexOf("```");
        if (end < 0) {
          if (final) this.pos = this.buf.length;
          break;
        }
        this.pos += end + 3;
        this.inCode = false;
        continue;
      }
      const fence = rest.indexOf("```");
      const m = (final ? /[.!?…](\s|$)|\n/ : /[.!?…]\s|\n/).exec(rest);
      const boundary = m ? m.index + m[0].length : -1;
      if (fence >= 0 && (boundary < 0 || fence < boundary)) {
        push(rest.slice(0, fence));
        out.push(CODE_NOTE);
        // salta el resto de la línea de apertura (```lenguaje)
        const nl = rest.indexOf("\n", fence);
        this.pos += nl >= 0 ? nl + 1 : fence + 3;
        this.inCode = true;
        continue;
      }
      if (boundary < 0) {
        if (final) {
          push(rest);
          this.pos = this.buf.length;
        }
        break;
      }
      push(rest.slice(0, boundary));
      this.pos += boundary;
    }
    // Evita anunciar el código dos veces seguidas.
    return out.filter((s, i) => !(s === CODE_NOTE && out[i - 1] === CODE_NOTE));
  }
}

/** Agrupa frases cortas para no hacer demasiadas peticiones a la voz de OpenAI. */
export function groupSentences(sentences: string[], min = 160): string[] {
  const out: string[] = [];
  let cur = "";
  for (const s of sentences) {
    cur = cur ? `${cur} ${s}` : s;
    if (cur.length >= min) {
      out.push(cur);
      cur = "";
    }
  }
  if (cur) out.push(cur);
  return out;
}

/** Voces de Windows en español, ordenadas (las locales primero). */
export function spanishVoices(
  all: SpeechSynthesisVoice[],
): SpeechSynthesisVoice[] {
  return all
    .filter((v) => v.lang.toLowerCase().startsWith("es"))
    .sort(
      (a, b) =>
        Number(b.localService) - Number(a.localService) ||
        a.name.localeCompare(b.name),
    );
}
