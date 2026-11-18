// Renderizado seguro y mínimo: bloques de código, `código en línea` y **negritas**.
// No usa innerHTML, así que el texto del modelo no puede inyectar HTML.
import { useState, type ReactNode } from 'react';

export type Segment = { kind: 'text'; text: string } | { kind: 'code'; lang: string; code: string };

export function splitSegments(src: string): Segment[] {
  const out: Segment[] = [];
  const re = /(```|~~~~)([\w+#.-]*)[^\n]*\n([\s\S]*?)(?:\n\1|$)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src))) {
    if (m.index > last) out.push({ kind: 'text', text: src.slice(last, m.index) });
    out.push({ kind: 'code', lang: m[2] || '', code: m[3] });
    last = re.lastIndex;
  }
  if (last < src.length) out.push({ kind: 'text', text: src.slice(last) });
  return out.filter((s) => s.kind === 'code' || s.text.length > 0);
}

function inline(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const re = /(`[^`\n]+`|\*\*[^*\n]+\*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) nodes.push(text.slice(last, m.index));
    const tok = m[0];
    nodes.push(tok.startsWith('`') ? <code key={k++}>{tok.slice(1, -1)}</code> : <strong key={k++}>{tok.slice(2, -2)}</strong>);
    last = re.lastIndex;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

function CodeBlock({ lang, code }: { lang: string; code: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="code-block">
      <div className="code-head">
        <span>{lang || 'código'}</span>
        <button
          onClick={async () => {
            await navigator.clipboard.writeText(code);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          }}
        >
          {copied ? '¡Copiado!' : 'Copiar'}
        </button>
      </div>
      <pre>
        <code>{code}</code>
      </pre>
    </div>
  );
}

export function Markdown({ text }: { text: string }) {
  return (
    <>
      {splitSegments(text).map((s, i) =>
        s.kind === 'code' ? (
          <CodeBlock key={i} lang={s.lang} code={s.code} />
        ) : (
          <p key={i} className="md-text">
            {inline(s.text)}
          </p>
        ),
      )}
    </>
  );
}
