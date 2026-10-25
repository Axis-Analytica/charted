import { ArrowDownWideNarrow, ArrowUpNarrowWide, Check, FunnelX, Minus, Search, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "../../bridge";
import type { Column } from "../../bridge/generated/Column";
import type { DistinctValues } from "../../bridge/generated/DistinctValues";
import type { Filter } from "../../bridge/generated/Filter";
import type { FilterOp } from "../../bridge/generated/FilterOp";
import type { ViewSpec } from "../../bridge/generated/ViewSpec";
import { formatValue } from "../../format";
import { formatNumber, useLanguage, useT } from "../../i18n";
import { Button } from "../../kit/Button";
import { Spinner } from "../../kit/misc";
import { dataJson } from "../../state/dataview";
import { DataTypeIcon } from "../icons";
import { isNumeric } from "./DataGrid";

const LIMIT = 1000;
/** Map key for blank, which has no text form. */
const BLANK = "\u0000blank";
const keyOf = (k: string | null) => k ?? BLANK;

type Mode = "all" | "include" | "exclude";

const TEXT_OPS: FilterOp[] = ["contains", "notContains", "startsWith", "endsWith", "equals", "notEquals", "isBlank", "isNotBlank"];
const ORDERED_OPS: FilterOp[] = ["equals", "notEquals", "greater", "greaterOrEqual", "less", "lessOrEqual", "between", "isBlank", "isNotBlank"];

function opLabel(op: FilterOp, t: (s: string) => string): string {
  switch (op) {
    case "equals": return t("is");
    case "notEquals": return t("is not");
    case "contains": return t("contains");
    case "notContains": return t("does not contain");
    case "startsWith": return t("starts with");
    case "endsWith": return t("ends with");
    case "less": return t("is less than");
    case "lessOrEqual": return t("is at most");
    case "greater": return t("is greater than");
    case "greaterOrEqual": return t("is at least");
    case "between": return t("is between");
    case "isBlank": return t("is blank");
    case "isNotBlank": return t("is not blank");
  }
}

export interface FilterMenuProps {
  column: Column;
  /** The view with this column's own filter (the menu lists values ignoring it). */
  view: ViewSpec;
  anchor: { x: number; y: number; width: number; height: number };
  onSort(descending: boolean | null, add: boolean): void;
  onFilter(filter: Filter | null): void;
  onClose(): void;
}

export function FilterMenu({ column, view, anchor, onSort, onFilter, onClose }: FilterMenuProps) {
  const { t, tn } = useT();
  const locale = useLanguage((s) => s.locale);
  const ref = useRef<HTMLDivElement>(null);
  const current = view.filters.find((f) => f.column === column.id)?.filter ?? null;
  const sortKey = view.sort.find((k) => k.column === column.id);
  const ordered = column.dataType !== "String" && column.dataType !== "Boolean";
  const ops = ordered ? ORDERED_OPS : TEXT_OPS;

  const [tab, setTab] = useState<"values" | "condition">(current?.kind === "condition" ? "condition" : "values");
  const [search, setSearch] = useState("");
  const [list, setList] = useState<DistinctValues | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>(current?.kind === "values" ? (current.exclude ? "exclude" : "include") : "all");
  const [picked, setPicked] = useState<Set<string>>(() => new Set(current?.kind === "values" ? current.values.map(keyOf) : []));
  const [op, setOp] = useState<FilterOp>(current?.kind === "condition" ? current.op : ops[0]!);
  const [value, setValue] = useState(current?.kind === "condition" ? (current.value ?? "") : "");
  const [value2, setValue2] = useState(current?.kind === "condition" ? (current.value2 ?? "") : "");

  // Place below the header, kept inside the window.
  const [pos, setPos] = useState({ left: anchor.x, top: anchor.y + anchor.height });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    setPos({
      left: Math.max(8, Math.min(anchor.x + anchor.width - w, window.innerWidth - w - 8, anchor.x)),
      top: Math.max(8, Math.min(anchor.y + anchor.height + 2, window.innerHeight - h - 8)),
    });
  }, [anchor, tab]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", onDown, true);
    return () => window.removeEventListener("mousedown", onDown, true);
  }, [onClose]);

  useEffect(() => {
    let live = true;
    setLoading(true);
    const timer = setTimeout(() => {
      dataJson<DistinctValues>({ type: "distinct", view, column: column.id, search: search || null, limit: LIMIT })
        .then((d) => live && (setList(d), setError(null)))
        .catch((e) => live && setError(errorMessage(e)))
        .finally(() => live && setLoading(false));
    }, search ? 150 : 0);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [view, column.id, search]);

  const isChecked = (k: string) => (mode === "all" ? true : mode === "include" ? picked.has(k) : !picked.has(k));
  const toggle = (k: string) => {
    if (mode === "all") {
      setMode("exclude");
      setPicked(new Set([k]));
      return;
    }
    const next = new Set(picked);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    setPicked(next);
  };
  const shown = list?.values ?? [];
  const checkedShown = shown.filter((v) => isChecked(keyOf(v.key))).length;
  const allState = checkedShown === 0 ? "none" : checkedShown === shown.length ? "all" : "some";
  const toggleAll = () => {
    if (search) {
      // Within search results, (un)check just the listed values.
      const keys = shown.map((v) => keyOf(v.key));
      const want = allState !== "all";
      const next = new Set(picked);
      const baseMode = mode === "all" ? "exclude" : mode;
      for (const k of keys) {
        const inSet = baseMode === "include" ? want : !want;
        if (inSet) next.add(k);
        else next.delete(k);
      }
      setMode(baseMode);
      setPicked(next);
      return;
    }
    setMode(allState === "all" ? "include" : "all");
    setPicked(new Set());
  };

  const valuesFilter = (): Filter | null | "nothing" => {
    const fromKey = (k: string) => (k === BLANK ? null : k);
    if (search) {
      // Searching then applying keeps exactly the checked results, as in Excel.
      const keys = shown.filter((v) => isChecked(keyOf(v.key))).map((v) => v.key);
      return keys.length ? { kind: "values", values: keys, exclude: false } : "nothing";
    }
    if (mode === "all") return null;
    if (mode === "exclude") return picked.size ? { kind: "values", values: [...picked].map(fromKey), exclude: true } : null;
    return picked.size ? { kind: "values", values: [...picked].map(fromKey), exclude: false } : "nothing";
  };
  const needsValue = op !== "isBlank" && op !== "isNotBlank";
  const conditionFilter = (): Filter | "nothing" =>
    needsValue && (!value.trim() || (op === "between" && !value2.trim())) ? "nothing" : { kind: "condition", op, value: needsValue ? value : null, value2: op === "between" ? value2 : null };
  const pending = tab === "values" ? valuesFilter() : conditionFilter();
  const apply = () => {
    if (pending === "nothing") return;
    onFilter(pending);
    onClose();
  };

  const placeholder = useMemo(() => {
    if (column.dataType === "Date") return "2025-01-31";
    if (column.dataType === "DateTime" || column.dataType === "DateTimeTz") return "2025-01-31 09:00";
    if (isNumeric(column.dataType)) return formatNumber(1234.5, { useGrouping: false });
    return t("Value");
  }, [column.dataType, t]);

  return (
    <div
      ref={ref}
      className="filter-menu"
      role="dialog"
      aria-label={t("Sort and filter %1", column.name)}
      style={{ left: pos.left, top: pos.top }}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onClose();
        if (e.key === "Enter" && (e.target as HTMLElement).tagName !== "BUTTON") apply();
      }}
    >
      <div className="fm-head">
        <DataTypeIcon type={column.dataType} />
        <span className="fm-title">{column.name}</span>
        <button type="button" className="fm-close" aria-label={t("Close")} onClick={onClose}>
          <X size={14} />
        </button>
      </div>
      <div className="fm-items" role="menu">
        <button type="button" role="menuitem" className={`fm-item ${sortKey && !sortKey.descending ? "is-on" : ""}`} onClick={(e) => (onSort(false, e.shiftKey), onClose())}>
          <ArrowUpNarrowWide size={14} /> {ordered ? t("Sort ascending") : t("Sort A to Z")}
        </button>
        <button type="button" role="menuitem" className={`fm-item ${sortKey?.descending ? "is-on" : ""}`} onClick={(e) => (onSort(true, e.shiftKey), onClose())}>
          <ArrowDownWideNarrow size={14} /> {ordered ? t("Sort descending") : t("Sort Z to A")}
        </button>
        {sortKey && (
          <button type="button" role="menuitem" className="fm-item" onClick={() => (onSort(null, true), onClose())}>
            <Minus size={14} /> {t("Remove from sort")}
          </button>
        )}
        <div className="fm-hint">{t("Shift-click a header, or hold Shift here, to sort by several columns.")}</div>
        <button type="button" role="menuitem" className="fm-item" disabled={!current} onClick={() => (onFilter(null), onClose())}>
          <FunnelX size={14} /> {t("Clear filter")}
        </button>
      </div>
      <div className="fm-tabs" role="tablist">
        <button type="button" role="tab" aria-selected={tab === "values"} onClick={() => setTab("values")}>
          {t("Values")}
        </button>
        <button type="button" role="tab" aria-selected={tab === "condition"} onClick={() => setTab("condition")}>
          {t("Condition")}
        </button>
      </div>
      {tab === "values" ? (
        <div className="fm-values">
          <label className="fm-search">
            <Search size={13} />
            <input autoFocus value={search} placeholder={t("Search values")} aria-label={t("Search values")} onChange={(e) => setSearch(e.target.value)} />
            {loading && <Spinner size={12} />}
          </label>
          {error ? (
            <div className="fm-error">{error}</div>
          ) : (
            <ul className="fm-list" role="listbox" aria-multiselectable aria-label={t("Values")}>
              {shown.length > 0 && (
                <li>
                  <button type="button" className="fm-check is-all" role="option" aria-selected={allState === "all"} onClick={toggleAll}>
                    <span className={`fm-box is-${allState}`}>{allState === "all" ? <Check size={11} /> : allState === "some" ? <Minus size={11} /> : null}</span>
                    <span className="fm-value">{search ? t("(Select all search results)") : t("(Select all)")}</span>
                  </button>
                </li>
              )}
              {shown.map((v) => {
                const k = keyOf(v.key);
                const on = isChecked(k);
                return (
                  <li key={k}>
                    <button type="button" className="fm-check" role="option" aria-selected={on} onClick={() => toggle(k)}>
                      <span className={`fm-box is-${on ? "all" : "none"}`}>{on && <Check size={11} />}</span>
                      <span className={`fm-value ${v.key === null ? "is-blank" : ""}`}>{v.key === null ? t("(Blank)") : formatValue(v.value, column.dataType, column.formatString, locale) || v.key}</span>
                      <span className="fm-count tabular">{formatNumber(v.count)}</span>
                    </button>
                  </li>
                );
              })}
              {!loading && shown.length === 0 && <li className="fm-empty">{t("No matching values")}</li>}
            </ul>
          )}
          {list && list.total > shown.length && <div className="fm-note">{t("Showing the %1 most common of %2 values. Search to find others.", formatNumber(shown.length), formatNumber(list.total))}</div>}
        </div>
      ) : (
        <div className="fm-condition">
          <label className="field">
            <span className="field-label">{t("Show rows where %1", column.name)}</span>
            <select className="select" value={op} onChange={(e) => setOp(e.target.value as FilterOp)} aria-label={t("Condition")}>
              {ops.map((o) => (
                <option key={o} value={o}>
                  {opLabel(o, t)}
                </option>
              ))}
            </select>
          </label>
          {needsValue && <input className="input" autoFocus value={value} placeholder={placeholder} aria-label={t("Value")} onChange={(e) => setValue(e.target.value)} />}
          {op === "between" && (
            <>
              <span className="fm-and">{t("and")}</span>
              <input className="input" value={value2} placeholder={placeholder} aria-label={t("Upper value")} onChange={(e) => setValue2(e.target.value)} />
            </>
          )}
          {!ordered && <div className="fm-note">{t("Text comparisons ignore case.")}</div>}
        </div>
      )}
      <div className="fm-foot">
        <span className="fm-foot-info">{tab === "values" && list ? tn("%n values", list.total, "%n value") : ""}</span>
        <Button size="sm" variant="ghost" onPress={onClose}>
          {t("Cancel")}
        </Button>
        <Button size="sm" variant="primary" isDisabled={pending === "nothing"} onPress={apply}>
          {t("Apply")}
        </Button>
      </div>
    </div>
  );
}
