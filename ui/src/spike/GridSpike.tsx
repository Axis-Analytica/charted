import "@glideapps/glide-data-grid/dist/index.css";
import { DataEditor, type DataEditorRef, type GridCell, GridCellKind, type GridColumn, type Item } from "@glideapps/glide-data-grid";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const ROWS = 1_000_000;
const columns: GridColumn[] = [
  { title: "OrderNumber", id: "o", width: 130 },
  { title: "Date", id: "d", width: 120 },
  { title: "Store", id: "s", width: 160 },
  { title: "Product", id: "p", width: 220 },
  { title: "Quantity", id: "q", width: 90 },
  { title: "UnitPrice", id: "u", width: 110 },
  { title: "Amount", id: "a", width: 120 },
  { title: "Channel", id: "c", width: 100 },
];
const stores = ["Harbor St", "Pine Ridge", "Northgate", "Old Mill", "Bayfront", "Online"];
const products = ["Trail Runner 2", "Alpine Shell Jacket", "Merino Base Layer", "Summit Pack 40L", "Camp Stove Mini"];

// Cheap deterministic pseudo-random value per (row, salt).
function hash(row: number, salt: number): number {
  let x = (Math.imul(row, 2654435761) + Math.imul(salt, 40503)) >>> 0;
  x ^= x >>> 15;
  x = Math.imul(x, 2246822519) >>> 0;
  x ^= x >>> 13;
  return x / 4294967295;
}

/** Scrolls through all rows over a few seconds so the frame meter measures real work. */
function useAutoScroll(ref: React.RefObject<DataEditorRef | null>, enabled: boolean) {
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (!enabled) return;
    let frame = 0;
    let raf = 0;
    const step = () => {
      frame++;
      const row = Math.min(ROWS - 1, frame * 1500 + (frame % 7) * 13);
      ref.current?.scrollTo(frame % 8, row, "both", 0, 0, { vAlign: "start" });
      if (row < ROWS - 1) raf = requestAnimationFrame(step);
      else setDone(true);
    };
    const t = setTimeout(() => (raf = requestAnimationFrame(step)), 800);
    return () => {
      clearTimeout(t);
      cancelAnimationFrame(raf);
    };
  }, [enabled, ref]);
  return done;
}

export function GridSpike() {
  const ref = useRef<DataEditorRef>(null);
  const auto = import.meta.env.VITE_SPIKE_AUTO === "1";
  const done = useAutoScroll(ref, auto);
  const fmt = useMemo(() => new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" }), []);
  const getCellContent = useCallback(
    ([col, row]: Item): GridCell => {
      const q = 1 + Math.floor(hash(row, 1) * 5);
      const u = 10 + Math.floor(hash(row, 2) * 300);
      switch (col) {
        case 0:
          return { kind: GridCellKind.Text, data: `SO${100000 + row}`, displayData: `SO${100000 + row}`, allowOverlay: false };
        case 1: {
          const d = new Date(Date.UTC(2022, 0, 1) + Math.floor(row / 700) * 86400000).toISOString().slice(0, 10);
          return { kind: GridCellKind.Text, data: d, displayData: d, allowOverlay: false };
        }
        case 2: {
          const s = stores[Math.floor(hash(row, 3) * stores.length)] ?? "";
          return { kind: GridCellKind.Text, data: s, displayData: s, allowOverlay: false };
        }
        case 3: {
          const p = products[Math.floor(hash(row, 4) * products.length)] ?? "";
          return { kind: GridCellKind.Text, data: p, displayData: p, allowOverlay: false };
        }
        case 4:
          return { kind: GridCellKind.Number, data: q, displayData: String(q), allowOverlay: false, contentAlign: "right" };
        case 5:
          return { kind: GridCellKind.Number, data: u, displayData: fmt.format(u), allowOverlay: false, contentAlign: "right" };
        case 6:
          return { kind: GridCellKind.Number, data: q * u, displayData: fmt.format(q * u), allowOverlay: false, contentAlign: "right" };
        default: {
          const c = hash(row, 5) > 0.7 ? "Online" : "Store";
          return { kind: GridCellKind.Text, data: c, displayData: c, allowOverlay: false };
        }
      }
    },
    [fmt],
  );
  return (
    <div className="fill">
      {auto && <p className="note" data-testid="autoscroll">{done ? "auto-scroll finished (1M rows)" : "auto-scrolling…"}</p>}
      <DataEditor ref={ref} columns={columns} rows={ROWS} getCellContent={getCellContent} rowMarkers="number" smoothScrollX smoothScrollY width="100%" height="100%" />
    </div>
  );
}
