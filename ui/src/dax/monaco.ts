// Registers DAX with Monaco once: syntax colours, brackets and comments, and the
// language service (completion, signature help, hover, formatting) from the worker.
// The theme is built from the app's own colour tokens, so it follows Omarchy's.
import * as monaco from "monaco-editor/editor/editor.api";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
// The bare API has no editor features; load the ones a formula editor uses.
import "monaco-editor/features/bracketMatching/register";
import "monaco-editor/features/clipboard/register";
import "monaco-editor/features/codeEditor/register";
import "monaco-editor/features/codicon/register";
import "monaco-editor/features/comment/register";
import "monaco-editor/features/cursorUndo/register";
import "monaco-editor/features/format/register";
import "monaco-editor/features/gotoError/register";
import "monaco-editor/features/hover/register";
import "monaco-editor/features/linesOperations/register";
import "monaco-editor/features/multicursor/register";
import "monaco-editor/features/parameterHints/register";
import "monaco-editor/features/snippet/register";
import "monaco-editor/features/suggest/register";
import "monaco-editor/editor/contrib/suggest/browser/suggestController";
import "monaco-editor/features/wordOperations/register";
import { dax, splitFormula } from "./service";

/** Per editor model: the calculated column's table, when editing one. */
export const homes = new WeakMap<monaco.editor.ITextModel, string | null>();

let registered = false;

const KEYWORDS = ["VAR", "RETURN", "TRUE", "FALSE", "IN", "NOT", "ASC", "DESC"];

/** The offset where the expression starts and the expression text. */
function expression(model: monaco.editor.ITextModel) {
  const text = model.getValue();
  const { exprStart } = splitFormula(text);
  return { text: text.slice(exprStart), start: exprStart };
}

function toRange(model: monaco.editor.ITextModel, start: number, end: number): monaco.IRange {
  const a = model.getPositionAt(start);
  const b = model.getPositionAt(end);
  return { startLineNumber: a.lineNumber, startColumn: a.column, endLineNumber: b.lineNumber, endColumn: b.column };
}

const KIND: Record<string, monaco.languages.CompletionItemKind> = {
  function: monaco.languages.CompletionItemKind.Function,
  table: monaco.languages.CompletionItemKind.Class,
  column: monaco.languages.CompletionItemKind.Field,
  measure: monaco.languages.CompletionItemKind.Value,
  variable: monaco.languages.CompletionItemKind.Variable,
  keyword: monaco.languages.CompletionItemKind.Keyword,
};

export function registerDax() {
  if (registered) return;
  registered = true;
  (self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = { getWorker: () => new EditorWorker() };

  monaco.languages.register({ id: "dax" });
  monaco.languages.setLanguageConfiguration("dax", {
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["(", ")"],
      ["{", "}"],
      ["[", "]"],
    ],
    autoClosingPairs: [
      { open: "(", close: ")" },
      { open: "{", close: "}" },
      { open: "[", close: "]", notIn: ["string"] },
      { open: '"', close: '"', notIn: ["string"] },
      { open: "'", close: "'", notIn: ["string"] },
    ],
    wordPattern: /[A-Za-z_][\w.]*/,
  });
  monaco.languages.setMonarchTokensProvider("dax", {
    ignoreCase: true,
    keywords: KEYWORDS,
    tokenizer: {
      root: [
        [/\/\/.*$/, "comment"],
        [/--.*$/, "comment"],
        [/\/\*/, "comment", "@comment"],
        [/"([^"]|"")*"/, "string"],
        [/'([^']|'')*'/, "type"],
        [/\[([^\]]|\]\])*\]/, "variable"],
        [/\d+(\.\d+)?([eE][-+]?\d+)?/, "number"],
        [/[A-Za-z_][\w.]*(?=\s*\()/, "function"],
        [/[A-Za-z_][\w.]*/, { cases: { "@keywords": "keyword", "@default": "type" } }],
        [/[-+*/^&=<>|!]+/, "operator"],
      ],
      comment: [
        [/[^*]+/, "comment"],
        [/\*\//, "comment", "@pop"],
        [/\*/, "comment"],
      ],
    },
  });

  monaco.languages.registerCompletionItemProvider("dax", {
    triggerCharacters: ["[", "'"],
    async provideCompletionItems(model, position) {
      const { text, start } = expression(model);
      const offset = model.getOffsetAt(position) - start;
      if (offset < 0) return { suggestions: [] };
      const items = await dax.complete(text, offset, homes.get(model) ?? null);
      return {
        suggestions: items.map((c) => ({
          label: c.label,
          kind: KIND[c.kind] ?? monaco.languages.CompletionItemKind.Text,
          detail: c.detail,
          documentation: c.documentation ? { value: c.documentation } : undefined,
          insertText: c.insertText,
          range: toRange(model, c.replace.start + start, c.replace.end + start),
          sortText: c.sort,
          // Typed text such as "sum" should match "SUM ( ".
          filterText: c.label,
          command: c.kind === "function" ? { id: "editor.action.triggerParameterHints", title: "" } : undefined,
        })),
      };
    },
  });
  monaco.languages.registerSignatureHelpProvider("dax", {
    signatureHelpTriggerCharacters: ["(", ","],
    signatureHelpRetriggerCharacters: [","],
    async provideSignatureHelp(model, position) {
      const { text, start } = expression(model);
      const s = await dax.signature(text, model.getOffsetAt(position) - start);
      if (!s) return null;
      return {
        value: {
          signatures: [{ label: s.label, documentation: s.documentation, parameters: s.params.map((p) => ({ label: p })) }],
          activeSignature: 0,
          activeParameter: s.active,
        },
        dispose() {},
      };
    },
  });
  monaco.languages.registerHoverProvider("dax", {
    async provideHover(model, position) {
      const { text, start } = expression(model);
      const h = await dax.hover(text, model.getOffsetAt(position) - start, homes.get(model) ?? null);
      if (!h) return null;
      return { contents: [{ value: h.contents }], range: toRange(model, h.range.start + start, h.range.end + start) };
    },
  });
  monaco.languages.registerDocumentFormattingEditProvider("dax", {
    async provideDocumentFormattingEdits(model) {
      const { text, start } = expression(model);
      const formatted = await dax.format(text);
      const head = model.getValue().slice(0, start).trimEnd();
      // A multi-line body starts on the line after `Name =`.
      const value = head ? (formatted.includes("\n") ? `${head}\n${formatted}` : `${head} ${formatted}`) : formatted;
      return [{ range: model.getFullModelRange(), text: value }];
    },
  });
}

/** A colour token as #rrggbb, whatever CSS syntax it is written in. */
function hex(name: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888888";
  const c = document.createElement("canvas");
  c.width = c.height = 1;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (!ctx) return "#888888";
  ctx.fillStyle = value;
  ctx.fillRect(0, 0, 1, 1);
  const d = ctx.getImageData(0, 0, 1, 1).data;
  return `#${[d[0], d[1], d[2]].map((x) => (x ?? 0).toString(16).padStart(2, "0")).join("")}`;
}

/** (Re)defines the "charted" theme from the current colour tokens. */
export function applyTheme(dark: boolean) {
  const bare = (n: string) => hex(n).slice(1);
  monaco.editor.defineTheme("charted", {
    base: dark ? "vs-dark" : "vs",
    inherit: true,
    rules: [
      { token: "", foreground: bare("--fg") },
      { token: "keyword", foreground: bare("--accent-text"), fontStyle: "bold" },
      { token: "function", foreground: bare("--info") },
      { token: "type", foreground: bare("--fg-strong") },
      { token: "variable", foreground: bare("--success") },
      { token: "string", foreground: bare("--warning") },
      { token: "number", foreground: bare("--warning") },
      { token: "comment", foreground: bare("--fg-faint"), fontStyle: "italic" },
      { token: "operator", foreground: bare("--fg-muted") },
    ],
    colors: {
      "editor.background": hex("--bg-sunken"),
      "editor.foreground": hex("--fg"),
      "editorCursor.foreground": hex("--accent"),
      "editor.selectionBackground": hex("--selection"),
      "editor.lineHighlightBackground": hex("--bg-sunken"),
      "editorError.foreground": hex("--danger"),
      "editorBracketMatch.background": `${hex("--selection")}80`,
      "editorBracketMatch.border": "#00000000",
      "editorSuggestWidget.background": hex("--bg-overlay"),
      "editorSuggestWidget.border": hex("--border-strong"),
      "editorSuggestWidget.selectedBackground": hex("--bg-active"),
      "editorSuggestWidget.highlightForeground": hex("--accent-text"),
      "editorHoverWidget.background": hex("--bg-overlay"),
      "editorHoverWidget.border": hex("--border-strong"),
      "editorWidget.background": hex("--bg-overlay"),
      "editorWidget.border": hex("--border-strong"),
      focusBorder: "#00000000",
    },
  });
  monaco.editor.setTheme("charted");
}

export { monaco };
