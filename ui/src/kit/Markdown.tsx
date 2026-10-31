// A small, safe markdown renderer for the agent's replies and report text boxes. It
// builds React nodes (never HTML strings), so text can't inject markup. The subset:
// paragraphs and line breaks, # to ### headings, - and 1. lists, **bold**, *italic*
// (or _italic_), ~~struck~~, `code` and [links](https://…), with \ to escape a marker.
// Links show their address on hover; they don't open (charted opens no web pages but
// its own).
import { Fragment, type ReactNode } from "react";

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "bold" | "italic" | "strike"; children: Inline[] }
  | { kind: "link"; url: string; children: Inline[] };

export type Block =
  | { kind: "paragraph"; lines: Inline[][] }
  | { kind: "heading"; level: 1 | 2 | 3; children: Inline[] }
  | { kind: "list"; ordered: boolean; items: Inline[][] };

const MARKS: { open: string; kind: "bold" | "italic" | "strike" }[] = [
  { open: "**", kind: "bold" },
  { open: "__", kind: "bold" },
  { open: "~~", kind: "strike" },
  { open: "*", kind: "italic" },
  { open: "_", kind: "italic" },
];

/** Inline markup: emphasis nests; an unclosed marker is plain text. */
export function parseInline(s: string): Inline[] {
  const out: Inline[] = [];
  let text = "";
  const flush = () => {
    if (text) out.push({ kind: "text", text });
    text = "";
  };
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    if (c === "\\" && i + 1 < s.length && /[\\`*_~[\]()#-]/.test(s[i + 1] ?? "")) {
      text += s[i + 1];
      i += 2;
      continue;
    }
    if (c === "`") {
      const end = s.indexOf("`", i + 1);
      if (end > i + 1) {
        flush();
        out.push({ kind: "code", text: s.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }
    if (c === "[") {
      const close = closing(s, i + 1, "]");
      const m = close > 0 ? /^\(([^)\s]+)\)/.exec(s.slice(close + 1)) : null;
      if (m) {
        flush();
        out.push({ kind: "link", url: m[1] ?? "", children: parseInline(s.slice(i + 1, close)) });
        i = close + 1 + m[0].length;
        continue;
      }
    }
    const mark = MARKS.find((m) => s.startsWith(m.open, i));
    // _ inside a word (snake_case) isn't emphasis.
    const wordy = mark?.open[0] === "_" && i > 0 && /\w/.test(s[i - 1] ?? "");
    if (mark && !wordy && i + mark.open.length < s.length && s[i + mark.open.length] !== " ") {
      const end = closer(s, i + mark.open.length, mark.open);
      if (end > 0) {
        flush();
        out.push({ kind: mark.kind, children: parseInline(s.slice(i + mark.open.length, end)) });
        i = end + mark.open.length;
        continue;
      }
    }
    text += c;
    i++;
  }
  flush();
  return out;
}

function closing(s: string, from: number, ch: string): number {
  for (let i = from; i < s.length; i++) {
    if (s[i] === "\\") i++;
    else if (s[i] === ch) return i;
  }
  return -1;
}

/** The end of an emphasis run: the marker again, not after a space, and (for a single *
 * or _) not the start of a double one. */
function closer(s: string, from: number, open: string): number {
  for (let i = from; i < s.length; i++) {
    if (s[i] === "\\") {
      i++;
      continue;
    }
    if (s[i] === "`") {
      const end = s.indexOf("`", i + 1);
      if (end > 0) i = end;
      continue;
    }
    if (!s.startsWith(open, i) || s[i - 1] === " ") continue;
    // "***": a double marker closes on the last two, after the single one inside.
    if (open.length === 2 && s[i + 2] === open[0]) continue;
    if (open.length === 1 && s[i + 1] === open) {
      i++;
      continue;
    }
    if (open === "_" && /\w/.test(s[i + 1] ?? "")) continue;
    return i;
  }
  return -1;
}

const ITEM = /^\s{0,3}(?:([-*+])|(\d{1,3})[.)])\s+(.*)$/;

export function parseBlocks(src: string): Block[] {
  const blocks: Block[] = [];
  let para: Inline[][] = [];
  let list: { ordered: boolean; items: Inline[][] } | null = null;
  const end = () => {
    if (para.length) blocks.push({ kind: "paragraph", lines: para });
    if (list) blocks.push({ kind: "list", ...list });
    para = [];
    list = null;
  };
  for (const line of src.replace(/\r\n?/g, "\n").split("\n")) {
    const h = /^(#{1,3})\s+(.*)$/.exec(line);
    const item = ITEM.exec(line);
    if (!line.trim()) end();
    else if (h) {
      end();
      blocks.push({ kind: "heading", level: (h[1] ?? "#").length as 1 | 2 | 3, children: parseInline((h[2] ?? "").trim()) });
    } else if (item) {
      const ordered = !item[1];
      if (para.length || (list && list.ordered !== ordered)) end();
      list ??= { ordered, items: [] };
      list.items.push(parseInline(item[3] ?? ""));
    } else if (list && /^\s{2,}\S/.test(line)) {
      // An indented line continues the item above.
      list.items[list.items.length - 1]?.push({ kind: "text", text: " " }, ...parseInline(line.trim()));
    } else {
      if (list) end();
      para.push(parseInline(line));
    }
  }
  end();
  return blocks;
}

function inlines(xs: Inline[]): ReactNode {
  return xs.map((x, i) => {
    switch (x.kind) {
      case "text":
        return <Fragment key={i}>{x.text}</Fragment>;
      case "code":
        return <code key={i}>{x.text}</code>;
      case "bold":
        return <strong key={i}>{inlines(x.children)}</strong>;
      case "italic":
        return <em key={i}>{inlines(x.children)}</em>;
      case "strike":
        return <s key={i}>{inlines(x.children)}</s>;
      case "link":
        return (
          <span key={i} className="md-link" title={x.url}>
            {inlines(x.children)}
          </span>
        );
    }
  });
}

export function Markdown({ text, className }: { text: string; className?: string }) {
  return (
    <div className={className ? `md ${className}` : "md"}>
      {parseBlocks(text).map((b, i) => {
        if (b.kind === "heading") {
          const H = `h${b.level}` as "h1" | "h2" | "h3";
          return <H key={i}>{inlines(b.children)}</H>;
        }
        if (b.kind === "list") {
          const L = b.ordered ? "ol" : "ul";
          return (
            <L key={i}>
              {b.items.map((it, j) => (
                <li key={j}>{inlines(it)}</li>
              ))}
            </L>
          );
        }
        return (
          <p key={i}>
            {b.lines.map((l, j) => (
              <Fragment key={j}>
                {j > 0 && <br />}
                {inlines(l)}
              </Fragment>
            ))}
          </p>
        );
      })}
    </div>
  );
}
