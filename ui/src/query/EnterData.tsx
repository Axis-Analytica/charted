// Enter data (Power Query's typed-in table): a small grid of text cells with named,
// typed columns. Pasting tab-separated text (from a spreadsheet) fills it from the cell
// pasted into, adding rows and columns as needed; pasted into a column name, the first
// line names the columns.
import { Plus, X } from "lucide-react";
import { useState } from "react";
import type { DataType } from "../bridge/generated/DataType";
import type { Source } from "../bridge/generated/Source";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { TYPES } from "./transforms";

type TableSource = Extract<Source, { kind: "table" }>;

/** Tab-separated text as rows of cells (spreadsheets end the last line with a newline). */
export function parseTsv(text: string): string[][] {
  const lines = text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n");
  return lines.map((l) => l.split("\t"));
}

export function EnterDataDialog({ name, source, onDone }: { name: string; source: TableSource | null; onDone(r: { name: string; source: TableSource } | null): void }) {
  const { t } = useT();
  const [title, setTitle] = useState(name);
  const [columns, setColumns] = useState<[string, DataType][]>(source?.columns.length ? source.columns : [["Column1", "String"]]);
  const [rows, setRows] = useState<string[][]>(source?.rows.length ? source.rows.map((r) => r.map((v) => v ?? "")) : [[""]]);
  const [error, setError] = useState<string | null>(null);

  const width = columns.length;
  const fit = (r: string[]) => Array.from({ length: width }, (_, i) => r[i] ?? "");
  const freeName = (cols: [string, DataType][], i: number) => {
    let n = i + 1;
    while (cols.some((c) => c[0] === `Column${n}`)) n++;
    return `Column${n}`;
  };
  const addColumn = () => {
    setColumns((c) => [...c, [freeName(c, c.length), "String"]]);
    setRows((rs) => rs.map((r) => [...fit(r), ""]));
  };
  const removeColumn = (i: number) => {
    if (width === 1) return;
    setColumns((c) => c.filter((_, j) => j !== i));
    setRows((rs) => rs.map((r) => fit(r).filter((_, j) => j !== i)));
  };
  const setCell = (r: number, c: number, v: string) => setRows((rs) => rs.map((row, i) => (i === r ? fit(row).map((x, j) => (j === c ? v : x)) : row)));
  /** Pastes a block of cells at (row, column); row -1 is the column names. */
  const paste = (row: number, col: number, text: string) => {
    const block = parseTsv(text);
    const need = Math.max(width, col + Math.max(...block.map((b) => b.length)));
    const cols: [string, DataType][] = [...columns];
    while (cols.length < need) cols.push([freeName(cols, cols.length), "String"]);
    let data = block;
    if (row < 0) {
      block[0]!.forEach((n, j) => (cols[col + j] = [n.trim() || cols[col + j]![0], cols[col + j]![1]]));
      data = block.slice(1);
      row = 0;
    }
    const next = rows.map((r) => Array.from({ length: need }, (_, j) => r[j] ?? ""));
    data.forEach((cells, i) => {
      while (next.length <= row + i) next.push(Array.from({ length: need }, () => ""));
      cells.forEach((v, j) => (next[row + i]![col + j] = v));
    });
    setColumns(cols);
    setRows(next);
  };
  const onPaste = (row: number, col: number) => (e: React.ClipboardEvent) => {
    const text = e.clipboardData.getData("text/plain");
    if (!/[\t\n]/.test(text.replace(/\r?\n$/, ""))) return;
    e.preventDefault();
    paste(row, col, text);
  };
  const ok = () => {
    const names = columns.map((c) => c[0].trim());
    if (!title.trim()) return setError(t("The table needs a name."));
    if (names.some((n) => !n)) return setError(t("Every column needs a name."));
    if (new Set(names.map((n) => n.toLowerCase())).size !== names.length) return setError(t("Two columns have the same name."));
    // Blank rows at the end are left out.
    const kept = rows.map(fit);
    while (kept.length && kept[kept.length - 1]!.every((v) => v === "")) kept.pop();
    onDone({
      name: title.trim(),
      source: { kind: "table", columns: columns.map((c, i) => [names[i]!, c[1]]), rows: kept.map((r) => r.map((v) => (v === "" ? null : v))) },
    });
  };
  const stop = (e: React.KeyboardEvent) => e.stopPropagation();
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone(null)}
      title={t("Enter data")}
      width={760}
      modal
      footer={
        <>
          <Button variant="ghost" onPress={() => onDone(null)}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" onPress={ok}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="qe-form">
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          <input className="input" aria-label={t("Table name")} value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={stop} />
        </label>
        <div className="qe-enter-wrap">
          <table className="qe-enter" aria-label={t("Table data")}>
            <thead>
              <tr>
                <th className="qe-rownum" />
                {columns.map(([n, ty], c) => (
                  <th key={c}>
                    <div className="qe-enter-head">
                      <input className="input" aria-label={t("Name of column %1", c + 1)} value={n} onChange={(e) => setColumns((cs) => cs.map((x, j) => (j === c ? [e.target.value, x[1]] : x)))} onPaste={onPaste(-1, c)} onKeyDown={stop} />
                      <button type="button" className="qe-enter-x" aria-label={t("Remove column %1", n)} title={t("Remove column")} disabled={width === 1} onClick={() => removeColumn(c)}>
                        <X size={12} />
                      </button>
                    </div>
                    <select className="select" aria-label={t("Type of %1", n)} value={ty} onChange={(e) => setColumns((cs) => cs.map((x, j) => (j === c ? [x[0], e.target.value as DataType] : x)))}>
                      {TYPES.map((o) => (
                        <option key={o.value} value={o.value}>
                          {t(o.label)}
                        </option>
                      ))}
                    </select>
                  </th>
                ))}
                <th>
                  <button type="button" className="qe-enter-x" aria-label={t("Add column")} title={t("Add column")} onClick={addColumn}>
                    <Plus size={14} />
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i}>
                  <td className="qe-rownum">{i + 1}</td>
                  {fit(r).map((v, c) => (
                    <td key={c}>
                      <input
                        className="input"
                        aria-label={t("Row %1, %2", i + 1, columns[c]![0])}
                        value={v}
                        onChange={(e) => setCell(i, c, e.target.value)}
                        onPaste={onPaste(i, c)}
                        onKeyDown={(e) => {
                          stop(e);
                          // Enter on the last row starts a new one.
                          if (e.key === "Enter" && i === rows.length - 1) setRows((rs) => [...rs, fit([])]);
                        }}
                      />
                    </td>
                  ))}
                  <td>
                    <button type="button" className="qe-enter-x" aria-label={t("Remove row %1", i + 1)} title={t("Remove row")} disabled={rows.length === 1} onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}>
                      <X size={12} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="qe-enter-tools">
          <Button size="sm" variant="ghost" icon={<Plus size={13} />} onPress={() => setRows((rs) => [...rs, fit([])])}>
            {t("Add row")}
          </Button>
          <span className="qe-hint">{t("Paste cells from a spreadsheet to fill the table; pasted into a column name, the first line names the columns.")}</span>
        </div>
        {error && <p className="qe-error">{error}</p>}
      </div>
    </Dialog>
  );
}
