// The slicer: a field's values to choose from, which filters the other visuals on the
// page (as its interactions say). Styles: a list, a dropdown, a range between two
// values (numbers and dates), or a relative date ("last 3 months"). The choice is
// saved with the report and is one undoable step.
import { ChevronDown, Eraser, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DateUnit } from "../../bridge/generated/DateUnit";
import type { FilterKind } from "../../bridge/generated/FilterKind";
import type { Model } from "../../bridge/generated/Model";
import type { Page } from "../../bridge/generated/Page";
import type { RelativePeriod } from "../../bridge/generated/RelativePeriod";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { useApp } from "../../state/app";
import { columnOf, editVisual, projectionName } from "../../state/report";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { PERIODS, relativeLabel, UNITS } from "../relative";
import { fmt, projectionFormat } from "./common";

function setSelection(page: Page, visual: Visual, selection: FilterKind | null, label = "Slicer selection") {
  return useApp.getState().edit(label, editVisual(page.id, visual.id, (v) => ({ ...v, selection })));
}

export function Slicer({ visual, page, model, data }: { visual: Visual; page: Page; model: Model; data: VisualData | null }) {
  const { t } = useT();
  const def = visualDef("slicer");
  const field = visual.roles.field?.[0];
  const style = prop<string>(visual.objects, def, "slicer", "style");
  const showHeader = prop<boolean>(visual.objects, def, "header", "show");
  const sel = visual.selection ?? null;
  const active = !!sel && sel.kind !== "all";
  if (!field) return null;
  return (
    <div className="slicer" data-style={style}>
      {showHeader && (
        <div className="slicer-head">
          <span className="slicer-title">{projectionName(model, field, t)}</span>
          {active && (
            <button type="button" className="slicer-clear" aria-label={t("Clear selections")} title={t("Clear selections")} onClick={() => setSelection(page, visual, null, "Clear slicer")}>
              <Eraser size={13} />
            </button>
          )}
        </div>
      )}
      {style === "between" ? (
        <RangeSlicer visual={visual} page={page} model={model} data={data} />
      ) : style === "relative" ? (
        <RelativeSlicer visual={visual} page={page} />
      ) : (
        <ListSlicer visual={visual} page={page} model={model} data={data} dropdown={style === "dropdown"} />
      )}
    </div>
  );
}

function ListSlicer({ visual, page, model, data, dropdown }: { visual: Visual; page: Page; model: Model; data: VisualData | null; dropdown: boolean }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const def = visualDef("slicer");
  const field = visual.roles.field![0]!;
  const f = projectionFormat(model, field);
  const multi = prop<boolean>(visual.objects, def, "selection", "multi");
  const selectAll = prop<boolean>(visual.objects, def, "selection", "selectAll");
  const searchable = prop<boolean>(visual.objects, def, "selection", "search");
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const items = useMemo(() => {
    if (!data) return [];
    const keys = keysOf(data, "field:0");
    const vals = data.columns.get("field:0");
    return keys.map((key, r) => {
      const raw = vals?.get(r);
      return { key, label: raw === null || raw === undefined ? t("(Blank)") : fmt(raw, f, locale) };
    });
  }, [data, f, locale, t]);
  const chosen = visual.selection?.kind === "basic" && !visual.selection.exclude ? visual.selection.values : [];
  const isChosen = (k: string | null) => chosen.some((c) => c === k);
  const shown = search.trim() ? items.filter((i) => i.label.toLowerCase().includes(search.trim().toLowerCase())) : items;

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close, true);
    return () => window.removeEventListener("mousedown", close, true);
  }, [open]);

  const pick = (key: string | null, add: boolean) => {
    let next: (string | null)[];
    if (multi || add) next = isChosen(key) ? chosen.filter((c) => c !== key) : [...chosen, key];
    else next = chosen.length === 1 && isChosen(key) ? [] : [key];
    setSelection(page, visual, next.length ? { kind: "basic", values: next, exclude: false } : null);
  };

  const list = (
    <div className="slicer-list" role="listbox" aria-multiselectable={multi} aria-label={projectionName(model, field, t)}>
      {selectAll && !search && (
        <label className="slicer-item">
          <input type="checkbox" checked={chosen.length === 0} onChange={() => setSelection(page, visual, null, "Clear slicer")} />
          <span>{t("Select all")}</span>
        </label>
      )}
      {shown.map((i) => (
        <label
          key={String(i.key)}
          className={`slicer-item ${isChosen(i.key) ? "is-on" : ""}`}
          role="option"
          aria-selected={isChosen(i.key)}
          onClick={(e) => {
            e.preventDefault();
            pick(i.key, e.ctrlKey || e.metaKey);
          }}
        >
          <input type={multi ? "checkbox" : "checkbox"} checked={isChosen(i.key)} readOnly tabIndex={-1} />
          <span>{i.label}</span>
        </label>
      ))}
      {data && !shown.length && <div className="slicer-empty">{t("No values")}</div>}
    </div>
  );
  const searchBox = searchable && (
    <div className="slicer-search">
      <Search size={12} />
      <input value={search} placeholder={t("Search")} aria-label={t("Search")} onChange={(e) => setSearch(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
    </div>
  );

  if (!dropdown)
    return (
      <>
        {searchBox}
        {list}
      </>
    );
  const summary = chosen.length === 0 ? t("All") : chosen.length === 1 ? (items.find((i) => i.key === chosen[0])?.label ?? chosen[0] ?? t("(Blank)")) : t("Multiple selections");
  return (
    <div className="slicer-dropdown" ref={root}>
      <button type="button" className="slicer-dropdown-btn" aria-expanded={open} aria-haspopup="listbox" onClick={() => setOpen((o) => !o)}>
        <span>{summary}</span>
        <ChevronDown size={14} />
      </button>
      {open && (
        <div className="slicer-popover" onPointerDown={(e) => e.stopPropagation()}>
          {searchBox}
          {list}
        </div>
      )}
    </div>
  );
}

const isDateType = (t: string | undefined) => t === "Date" || t === "DateTime" || t === "DateTimeTz";

/** A date or number as the text a filter compares (dates as YYYY-MM-DD). */
function boundText(v: unknown, date: boolean): string {
  if (v === null || v === undefined) return "";
  if (date) {
    const d = typeof v === "number" || typeof v === "bigint" ? new Date(Number(v)) : new Date(String(v));
    return Number.isNaN(d.getTime()) ? String(v) : d.toISOString().slice(0, 10);
  }
  return String(typeof v === "bigint" ? Number(v) : v);
}

function RangeSlicer({ visual, page, model, data }: { visual: Visual; page: Page; model: Model; data: VisualData | null }) {
  const { t } = useT();
  const field = visual.roles.field![0]!;
  const col = columnOf(model, field.field);
  const date = isDateType(col?.dataType);
  const min = boundText(data?.columns.get("min")?.get(0), date);
  const max = boundText(data?.columns.get("max")?.get(0), date);
  const sel = visual.selection?.kind === "advanced" ? visual.selection.conditions : [];
  const lo = sel.find((c) => c.op === "greaterOrEqual")?.value ?? "";
  const hi = sel.find((c) => c.op === "lessOrEqual")?.value ?? "";
  const [from, setFrom] = useState(lo || min);
  const [to, setTo] = useState(hi || max);
  useEffect(() => setFrom(lo || min), [lo, min]);
  useEffect(() => setTo(hi || max), [hi, max]);
  const apply = (a: string, b: string) => {
    const conditions = [];
    if (a && a !== min) conditions.push({ op: "greaterOrEqual" as const, value: a, value2: null });
    if (b && b !== max) conditions.push({ op: "lessOrEqual" as const, value: b, value2: null });
    if (a === lo && b === hi) return;
    setSelection(page, visual, conditions.length ? { kind: "advanced", conditions, or: false } : null);
  };
  const num = (s: string) => (date ? new Date(s).getTime() : Number(s));
  const span = num(max) - num(min);
  const pos = (s: string) => {
    const p = ((num(s) - num(min)) / span) * 100;
    return span > 0 && Number.isFinite(p) ? Math.min(100, Math.max(0, p)) : 0;
  };
  const fromSlider = (pct: number) => {
    const v = num(min) + (span * pct) / 100;
    return date ? new Date(v).toISOString().slice(0, 10) : String(Math.round(v * 100) / 100);
  };
  const input = (value: string, set: (s: string) => void, label: string, commit: (s: string) => void) => (
    <input
      type={date ? "date" : "number"}
      value={value}
      aria-label={label}
      min={min}
      max={max}
      onChange={(e) => set(e.target.value)}
      onBlur={(e) => commit(e.target.value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") commit((e.target as HTMLInputElement).value);
      }}
    />
  );
  if (!data) return null;
  return (
    <div className="slicer-range">
      <div className="slicer-range-inputs">
        {input(from, setFrom, t("Start"), (s) => apply(s, to))}
        {input(to, setTo, t("End"), (s) => apply(from, s))}
      </div>
      <div className="slicer-track" style={{ ["--lo" as string]: `${pos(from)}%`, ["--hi" as string]: `${pos(to)}%` }}>
        <input type="range" min={0} max={100} step={0.1} value={pos(from)} aria-label={t("Start")} onChange={(e) => setFrom(fromSlider(Math.min(Number(e.target.value), pos(to))))} onPointerUp={() => apply(from, to)} onKeyUp={() => apply(from, to)} onPointerDown={(e) => e.stopPropagation()} />
        <input type="range" min={0} max={100} step={0.1} value={pos(to)} aria-label={t("End")} onChange={(e) => setTo(fromSlider(Math.max(Number(e.target.value), pos(from))))} onPointerUp={() => apply(from, to)} onKeyUp={() => apply(from, to)} onPointerDown={(e) => e.stopPropagation()} />
      </div>
    </div>
  );
}

function RelativeSlicer({ visual, page }: { visual: Visual; page: Page }) {
  const { t } = useT();
  const sel = visual.selection?.kind === "relativeDate" ? visual.selection : null;
  const [period, setPeriod] = useState<RelativePeriod>(sel?.period ?? "last");
  const [count, setCount] = useState(sel?.count ?? 1);
  const [unit, setUnit] = useState<DateUnit>(sel?.unit ?? "months");
  useEffect(() => {
    if (sel) {
      setPeriod(sel.period);
      setCount(sel.count);
      setUnit(sel.unit);
    }
  }, [sel]);
  const apply = (p: RelativePeriod, c: number, u: DateUnit) => setSelection(page, visual, { kind: "relativeDate", period: p, count: Math.max(1, Math.round(c) || 1), unit: u });
  return (
    <div className="slicer-relative">
      <select aria-label={t("Period")} value={period} onChange={(e) => (setPeriod(e.target.value as RelativePeriod), apply(e.target.value as RelativePeriod, count, unit))}>
        {PERIODS.map((p) => (
          <option key={p.value} value={p.value}>
            {t(p.label)}
          </option>
        ))}
      </select>
      {period !== "this" && (
        <input
          type="number"
          min={1}
          aria-label={t("Number")}
          value={count}
          onChange={(e) => setCount(Number(e.target.value))}
          onBlur={() => apply(period, count, unit)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") apply(period, count, unit);
          }}
        />
      )}
      <select aria-label={t("Unit")} value={unit} onChange={(e) => (setUnit(e.target.value as DateUnit), apply(period, count, e.target.value as DateUnit))}>
        {UNITS.map((u) => (
          <option key={u.value} value={u.value}>
            {t(u.label)}
          </option>
        ))}
      </select>
      <div className="slicer-relative-note">{sel ? relativeLabel(sel.period, sel.count, sel.unit, t) : t("Not filtering")}</div>
    </div>
  );
}
