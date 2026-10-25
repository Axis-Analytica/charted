import { invoke, isTauri } from "@tauri-apps/api/core";
import { tableFromIPC } from "apache-arrow";
import { useEffect, useState } from "react";

interface Result {
  rows: number;
  bytes: number;
  ipcMs: number;
  decodeMs: number;
  sum: number;
}

export function ArrowSpike() {
  const [res, setRes] = useState<Result | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (rows: number) => {
    setBusy(true);
    setErr(null);
    try {
      const t0 = performance.now();
      const buf = await invoke<ArrayBuffer>("spike_arrow", { rows });
      const t1 = performance.now();
      const table = tableFromIPC(new Uint8Array(buf));
      const amount = table.getChild("amount");
      let sum = 0;
      if (amount) for (let i = 0; i < amount.length; i++) sum += Number(amount.get(i));
      const t2 = performance.now();
      setRes({ rows: table.numRows, bytes: buf.byteLength, ipcMs: t1 - t0, decodeMs: t2 - t1, sum });
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    if (import.meta.env.VITE_SPIKE_AUTO === "1" && isTauri()) void run(1_000_000);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!isTauri()) return <p className="note">Arrow IPC needs the Tauri shell (run bin/dev).</p>;
  return (
    <div className="pad">
      <div className="row">
        <button type="button" disabled={busy} onClick={() => run(100_000)}>100k rows</button>
        <button type="button" disabled={busy} onClick={() => run(1_000_000)}>1M rows</button>
      </div>
      {err && <p className="error">{err}</p>}
      {res && (
        <dl className="stats" data-testid="arrow-result">
          <dt>Rows</dt><dd>{res.rows.toLocaleString()}</dd>
          <dt>Payload</dt><dd>{(res.bytes / 1048576).toFixed(1)} MiB</dd>
          <dt>DuckDB query + IPC</dt><dd>{res.ipcMs.toFixed(0)} ms</dd>
          <dt>Decode + scan</dt><dd>{res.decodeMs.toFixed(0)} ms</dd>
          <dt>Sum(amount)</dt><dd>{res.sum.toFixed(2)}</dd>
        </dl>
      )}
    </div>
  );
}
