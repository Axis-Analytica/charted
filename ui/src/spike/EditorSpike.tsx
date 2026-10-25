import * as monaco from "monaco-editor/editor/editor.api";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import { useEffect, useRef, useState } from "react";
import WasmWorker from "./wasm.worker.ts?worker";

(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

const SAMPLE = `Margin % =
VAR Revenue = SUM ( Sales[Amount] )
VAR Cost = SUMX ( Sales, Sales[Quantity] * RELATED ( Product[UnitCost] ) )
RETURN
    DIVIDE ( Revenue - Cost, Revenue )
`;

export function EditorSpike() {
  const host = useRef<HTMLDivElement>(null);
  const [wasm, setWasm] = useState("loading…");
  useEffect(() => {
    if (!host.current) return;
    const editor = monaco.editor.create(host.current, {
      value: SAMPLE,
      language: "plaintext",
      theme: "vs-dark",
      fontSize: 14,
      minimap: { enabled: false },
      automaticLayout: true,
    });
    const w = new WasmWorker();
    w.onmessage = (e: MessageEvent<{ mode: string; result: number }>) => setWasm(`add(40, 2) = ${e.data.result} via ${e.data.mode}`);
    w.onerror = (e) => setWasm(`worker error: ${e.message}`);
    w.postMessage(null);
    return () => {
      editor.dispose();
      w.terminate();
    };
  }, []);
  return (
    <div className="split">
      <p className="note" data-testid="wasm-result">WASM in module worker: {wasm}</p>
      <div ref={host} className="editor" />
    </div>
  );
}
