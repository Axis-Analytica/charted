// The date table wizard: a generated calendar (one row per day, with year, quarter,
// month, weekday and fiscal-year columns), marked as the date table and related to
// the model's date columns, so time intelligence works.
import { useEffect, useMemo, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { ColumnStats } from "../bridge/generated/ColumnStats";
import type { Model } from "../bridge/generated/Model";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";
import "../query/query.css";

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/** The year of a statistic's value: ISO text, epoch milliseconds or epoch days. */
function yearOf(v: unknown): number | null {
  if (typeof v === "string" && /^\d{4}/.test(v)) return Number(v.slice(0, 4));
  if (typeof v === "number") return new Date(Math.abs(v) > 1e9 ? v : v * 86_400_000).getUTCFullYear();
  return null;
}

export function DateTableDialog({ model, onClose }: { model: Model; onClose(): void }) {
  const { t } = useT();
  const taken = (n: string) => model.tables.some((x) => x.name.toLowerCase() === n.toLowerCase());
  const [name, setName] = useState(() => ["Date", "Calendar", "Dates"].find((n) => !taken(n)) ?? "Calendar 2");
  // Every date column outside date tables, ticked when nothing relates it to a date table yet.
  const columns = useMemo(
    () =>
      model.tables
        .filter((x) => !x.isDateTable)
        .flatMap((x) => x.columns.filter((c) => c.dataType === "Date").map((c) => ({ table: x, column: c }))),
    [model],
  );
  const [picked, setPicked] = useState<Set<string>>(() => new Set(columns.filter(({ column }) => !model.relationships.some((r) => r.fromColumn === column.id || r.toColumn === column.id)).map(({ column }) => column.id)));
  const now = new Date().getFullYear();
  const [from, setFrom] = useState(now - 2);
  const [to, setTo] = useState(now);
  const [fiscal, setFiscal] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The years of the picked columns' data.
  useEffect(() => {
    const chosen = columns.filter(({ column }) => picked.has(column.id));
    if (!chosen.length) return;
    let live = true;
    Promise.all(chosen.map(({ table, column }) => dataJson<ColumnStats>({ type: "stats", view: { table: table.id, sort: [], filters: [] }, column: column.id }).catch(() => null))).then((all) => {
      if (!live) return;
      const lo = all.map((s) => yearOf(s?.min)).filter((y): y is number => y !== null);
      const hi = all.map((s) => yearOf(s?.max)).filter((y): y is number => y !== null);
      if (lo.length) setFrom(Math.min(...lo));
      if (hi.length) setTo(Math.max(...hi));
    });
    return () => {
      live = false;
    };
  }, [columns, picked]);

  const problem = !name.trim() ? t("Name the date table.") : taken(name.trim()) ? t("There's already a table called %1.", name.trim()) : from > to ? t("The last year comes before the first.") : null;

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const state = await transport.rpc({
        type: "createDateTable",
        name: name.trim(),
        start: `${from}-01-01`,
        end: `${to}-12-31`,
        fiscalStart: fiscal,
        relate: columns.filter(({ column }) => picked.has(column.id)).map(({ table, column }) => [table.id, column.id] as [string, string]),
      });
      useApp.setState({ state });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("New date table")}
      width={520}
      footer={
        <>
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" onPress={create} isDisabled={!!problem || busy}>
            {busy ? t("Creating…") : t("Create")}
          </Button>
        </>
      }
    >
      <div className="qe-form">
        <p className="qe-hint">{t("One row per day, with year, quarter, month, weekday and fiscal-year columns. It is marked as the date table, so SAMEPERIODLASTYEAR, TOTALYTD and the other time-intelligence functions work.")}</p>
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} aria-label={t("Name")} />
        </label>
        <div className="qe-row">
          <label className="field">
            <span className="field-label">{t("First year")}</span>
            <input className="input tabular" type="number" min={1900} max={2200} value={from} onChange={(e) => setFrom(Number(e.target.value))} aria-label={t("First year")} />
          </label>
          <label className="field">
            <span className="field-label">{t("Last year")}</span>
            <input className="input tabular" type="number" min={1900} max={2200} value={to} onChange={(e) => setTo(Number(e.target.value))} aria-label={t("Last year")} />
          </label>
        </div>
        <label className="field">
          <span className="field-label">{t("Fiscal year starts in")}</span>
          <select className="select" value={fiscal} onChange={(e) => setFiscal(Number(e.target.value))} aria-label={t("Fiscal year starts in")}>
            {MONTHS.map((m, i) => (
              <option key={m} value={i + 1}>
                {t(m)}
              </option>
            ))}
          </select>
        </label>
        <div className="qe-columns">
          <span className="field-label">{t("Relate to")}</span>
          {columns.length ? (
            <div className="qe-columns-list" role="group" aria-label={t("Relate to")}>
              {columns.map(({ table, column }) => (
                <label key={column.id} className="check">
                  <input
                    type="checkbox"
                    checked={picked.has(column.id)}
                    onChange={(e) =>
                      setPicked((cur) => {
                        const next = new Set(cur);
                        if (e.target.checked) next.add(column.id);
                        else next.delete(column.id);
                        return next;
                      })
                    }
                  />
                  {`${table.name}[${column.name}]`}
                </label>
              ))}
            </div>
          ) : (
            <p className="qe-hint">{t("No date columns to relate. Change a column's type to Date to relate it.")}</p>
          )}
        </div>
        {(error ?? problem) && <p className="qe-error">{error ?? problem}</p>}
      </div>
    </Dialog>
  );
}
