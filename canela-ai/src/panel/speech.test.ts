import { describe, expect, it } from "vitest";
import { cleanForSpeech, groupSentences, SentenceSplitter } from "./speech";

describe("voz hablada", () => {
  it("limpia markdown, enlaces y emojis", () => {
    expect(
      cleanForSpeech("**Hola** 🐾 mira [la guía](https://x.com) o `npm i`"),
    ).toBe("Hola mira la guía o npm i");
    expect(cleanForSpeech("- uno\n- dos")).toBe("uno dos");
  });

  it("entrega frases a medida que llegan, sin cortar decimales", () => {
    const sp = new SentenceSplitter();
    expect(sp.feed("Hola, Edua")).toEqual([]);
    expect(sp.feed("rdo. La versión 3.5 es")).toEqual(["Hola, Eduardo."]);
    expect(sp.feed(" estable! Y tú")).toEqual(["La versión 3.5 es estable!"]);
    expect(sp.flush()).toEqual(["Y tú"]);
  });

  it("no lee bloques de código", () => {
    const sp = new SentenceSplitter();
    const out = [
      ...sp.feed('Usa esto:\n```js\nconsole.log("secreto");\n'),
      ...sp.feed("```\nListo."),
      ...sp.flush(),
    ];
    expect(out).toEqual([
      "Usa esto:",
      "Te dejé el código en el chat.",
      "Listo.",
    ]);
    expect(out.join(" ")).not.toContain("secreto");
  });

  it("agrupa frases cortas", () => {
    expect(
      groupSentences(["a".repeat(100), "b".repeat(100), "c"], 160),
    ).toEqual(["a".repeat(100) + " " + "b".repeat(100), "c"]);
  });
});

import { ttsEngineFor } from "./speech";
import { fmtMb } from "../lib/ipc";

describe("motor de voz v0.5", () => {
  it("Piper y Windows funcionan sin internet; OpenAI sólo si está permitido", () => {
    expect(ttsEngineFor("piper", false)).toBe("piper");
    expect(ttsEngineFor("local", false)).toBe("local");
    expect(ttsEngineFor("openai", false)).toBe("local");
    expect(ttsEngineFor("openai", true)).toBe("openai");
  });
  it("muestra tamaños de descarga", () => {
    expect(fmtMb(190_085_487)).toBe("190 MB");
    expect(fmtMb(1_200_000_000)).toBe("1,2 GB");
  });
});
