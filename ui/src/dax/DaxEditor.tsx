// A one-to-several-line DAX editor (Monaco with the WebAssembly language service).
// Enter commits and Shift+Enter starts a new line, unless a suggestion list is open;
// Escape cancels. Problems are underlined as you type.
import { useEffect, useImperativeHandle, useRef, type Ref } from "react";
import type { Model } from "../bridge/generated/Model";
import { useApp } from "../state/app";
import { applyTheme, homes, monaco, registerDax } from "./monaco";
import { dax, splitFormula, syncModel } from "./service";

export interface DaxEditorHandle {
  focus(): void;
  format(): void;
}

export interface ExtraProblem {
  message: string;
  /** Offsets in the expression (after `Name =`). */
  start: number;
  end: number;
}

interface Props {
  value: string;
  onChange(value: string): void;
  onCommit(): void;
  onCancel(): void;
  model: Model;
  /** A calculated column's table (its columns are in row context). */
  home?: string | null;
  label: string;
  /** Problems found elsewhere (the server's compile check), shown with the editor's own. */
  extra?: ExtraProblem | null;
  maxLines?: number;
  autoFocus?: boolean;
  ref?: Ref<DaxEditorHandle>;
}

const LINE = 20;

export function DaxEditor({ value, onChange, onCommit, onCancel, model, home = null, label, extra, maxLines = 8, autoFocus, ref }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  // Callbacks change every render; the editor's commands read the latest.
  const latest = useRef({ onChange, onCommit, onCancel });
  latest.current = { onChange, onCommit, onCancel };
  const themeKey = useApp((s) => JSON.stringify(s.env?.theme ?? null));
  const dark = useApp((s) => s.env?.theme.mode !== "light");

  useImperativeHandle(ref, () => ({
    focus: () => editor.current?.focus(),
    format: () => void editor.current?.getAction("editor.action.formatDocument")?.run(),
  }));

  useEffect(() => {
    registerDax();
    applyTheme(dark);
    const e = monaco.editor.create(host.current!, {
      value,
      language: "dax",
      theme: "charted",
      ariaLabel: label,
      // WebKitGTK has no EditContext; use the textarea input everywhere for one behaviour.
      editContext: false,
      fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--font-mono"),
      fontSize: 13,
      lineHeight: LINE,
      lineNumbers: "off",
      glyphMargin: false,
      folding: false,
      lineDecorationsWidth: 6,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      overviewRulerLanes: 0,
      hideCursorInOverviewRuler: true,
      renderLineHighlight: "none",
      wordWrap: "on",
      wrappingIndent: "indent",
      automaticLayout: true,
      fixedOverflowWidgets: true,
      scrollbar: { vertical: "auto", horizontal: "hidden", verticalScrollbarSize: 8, alwaysConsumeMouseWheel: false },
      suggest: { showWords: false, preview: true },
      quickSuggestions: { other: true, comments: false, strings: false },
      tabSize: 4,
      padding: { top: 4, bottom: 4 },
    });
    editor.current = e;
    homes.set(e.getModel()!, home);
    e.addCommand(monaco.KeyCode.Enter, () => latest.current.onCommit(), "!suggestWidgetVisible");
    e.addCommand(monaco.KeyMod.Shift | monaco.KeyCode.Enter, () => e.trigger("keyboard", "type", { text: "\n" }), "!suggestWidgetVisible");
    e.addCommand(monaco.KeyCode.Escape, () => latest.current.onCancel(), "!suggestWidgetVisible && !parameterHintsVisible");
    const fit = () => {
      const lines = Math.min(maxLines, Math.max(1, (e.getContentHeight() - 8) / LINE));
      host.current!.style.height = `${Math.ceil(lines) * LINE + 8}px`;
    };
    e.onDidContentSizeChange(fit);
    fit();
    e.onDidChangeModelContent(() => latest.current.onChange(e.getValue()));
    // Keys typed in the editor belong to it, not to the app's shortcuts.
    host.current!.addEventListener("keydown", (ev) => ev.stopPropagation());
    if (autoFocus) {
      e.focus();
      const m = e.getModel()!;
      e.setPosition(m.getPositionAt(m.getValueLength()));
    }
    return () => e.dispose();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Outside changes (another measure, an undo) replace the text.
  useEffect(() => {
    const e = editor.current;
    if (e && e.getValue() !== value) {
      e.setValue(value);
      const m = e.getModel()!;
      e.setPosition(m.getPositionAt(m.getValueLength()));
    }
  }, [value]);
  useEffect(() => {
    const m = editor.current?.getModel();
    if (m) homes.set(m, home);
  }, [home]);
  useEffect(() => applyTheme(dark), [themeKey, dark]);

  // Diagnostics from the language service, plus any from the server.
  useEffect(() => {
    syncModel(model);
    const e = editor.current;
    const m = e?.getModel();
    if (!m) return;
    let live = true;
    const { exprStart } = splitFormula(value);
    const expr = value.slice(exprStart);
    const mark = (p: { message: string; start: number; end: number }): monaco.editor.IMarkerData => {
      const a = m.getPositionAt(exprStart + p.start);
      const b = m.getPositionAt(exprStart + Math.max(p.end, p.start + 1));
      return { severity: monaco.MarkerSeverity.Error, message: p.message, startLineNumber: a.lineNumber, startColumn: a.column, endLineNumber: b.lineNumber, endColumn: b.column };
    };
    dax.diagnostics(expr, home).then((ds) => {
      if (!live) return;
      const all = ds.map(mark);
      if (extra && !ds.length) all.push(mark(extra));
      monaco.editor.setModelMarkers(m, "dax", expr.trim() ? all : []);
    });
    return () => {
      live = false;
    };
  }, [value, model, home, extra]);

  return <div ref={host} className="dax-editor" />;
}
