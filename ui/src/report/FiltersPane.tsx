// The Filters pane: filters on the selected visual, on this page and on all pages.
// Each filter card offers Basic (a value checklist), Advanced (one or two conditions)
// and, on visuals, Top N by a measure. Fields are dropped in from the Fields pane; a
// visual's own fields are listed so they can be filtered directly.
import { ChevronDown, Eraser, Search, X } from "lucide-react";
import { useEffect, useState } from "react";
import { errorMessage } from "../bridge";
import type { Condition } from "../bridge/generated/Condition";
import type { DataType } from "../bridge/generated/DataType";
import type { DistinctValues } from "../bridge/generated/DistinctValues";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { FilterKind } from "../bridge/generated/FilterKind";
import type { FilterOp } from "../bridge/generated/FilterOp";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { ReportFilter } from "../bridge/generated/ReportFilter";
import { formatValue } from "../format";
import { useLanguage, useT } from "../i18n";
import { Button } from "../kit/Button";
import { Spinner } from "../kit/misc";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";
import { columnOf, fieldKey, newId, projectionName, updateVisual, useReport, pageSettings } from "../state/report";
import { relativeLabel } from "./relative";
import { FIELD_MIME } from "./Canvas";

type Scope = "visual" | "page" | "report";

const TEXT_OPS: FilterOp[] = ["contains", "notContains", "startsWith", "equals", "notEquals", "isBlank", "isNotBlank"];
const NUM_OPS: FilterOp[] = ["equals", "notEquals", "greater", "greaterOrEqual", "less", "lessOrEqual", "isBlank", "isNotBlank"];

function opLabel(op: FilterOp, t: (s: string) => string): string {
  const labels: Record<FilterOp, string> = {
    equals: "is",
    notEquals: "is not",
    contains: "contains",
    notContains: "does not contain",
    startsWith: "starts with",
    endsWith: "ends with",
    less: "is less than",
    lessOrEqual: "is at most",
    greater: "is greater than",
    greaterOrEqual: "is at least",
    between: "is between",
    isBlank: "is blank",
    isNotBlank: "is not blank",
  };
  return t(labels[op]);
}

export function FiltersPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const selected = useReport((s) => s.selected);
  const visual = selected.length === 1 ? page.visuals.find((v) => v.id === selected[0]) : undefined;
  /** Rewrites one scope's filter list, starting from the latest model. */
  const save = (scope: Scope, label: string, fn: (filters: ReportFilter[]) => ReportFilter[]) =>
    edit(label, (m) => {
      const p = m.report.pages.find((x) => x.id === page.id);
      if (!p) return null;
      if (scope === "visual") {
        const v = visual && p.visuals.find((x) => x.id === visual.id);
        return v ? [updateVisual(page.id, { ...v, filters: fn(v.filters) })] : null;
      }
      if (scope === "page") return [{ op: "setPage", page: p.id, settings: { ...pageSettings(p), filters: fn(p.filters) } }];
      return [{ op: "setReportFilters", filters: fn(m.report.filters) }];
    });
  const listOf = (scope: Scope): ReportFilter[] => (scope === "visual" ? (visual?.filters ?? []) : scope === "page" ? page.filters : model.report.filters);

  const section = (scope: Scope, title: string) => {
    const filters = listOf(scope);
    // A visual's fields are offered as filters without being added.
    const implicit: ReportFilter[] =
      scope === "visual" && visual
        ? Object.values(visual.roles)
            .flat()
            .filter((p): p is Projection => !!p && !filters.some((f) => fieldKey(f.field.field) === fieldKey(p.field)))
            .map((p) => ({ id: `implicit-${fieldKey(p.field)}`, field: p, filter: { kind: "all" } }))
        : [];
    return (
      <Section
        key={scope}
        title={title}
        onDropField={(field) => {
          const col = columnOf(model, field);
          if (field.kind === "measure" && scope !== "visual") return;
          const p: Projection = { field, aggregation: null, displayName: null };
          if (!col && field.kind !== "measure") return;
          save(scope, "Add filter", (fs) => [...fs, { id: newId(), field: p, filter: { kind: "all" } }]);
        }}
      >
        {[...filters, ...implicit].map((f) => {
          const explicit = filters.some((x) => x.id === f.id);
          const replace = (kind: FilterKind) =>
            save(scope, "Change filter", (fs) =>
              fs.some((x) => x.id === f.id) ? fs.map((x) => (x.id === f.id ? { ...x, filter: kind } : x)) : [...fs, { ...f, id: newId(), filter: kind }],
            );
          return (
            <FilterCard
              key={f.id}
              model={model}
              filter={f}
              scope={scope}
              onChange={replace}
              onRemove={explicit ? () => save(scope, "Remove filter", (fs) => fs.filter((x) => x.id !== f.id)) : undefined}
            />
          );
        })}
      </Section>
    );
  };

  return (
    <section className="pane filters-pane" aria-label={t("Filters")}>
      <div className="pane-head">
        <span>{t("Filters")}</span>
      </div>
      <div className="pane-body">
        {visual && section("visual", t("Filters on this visual"))}
        {section("page", t("Filters on this page"))}
        {section("report", t("Filters on all pages"))}
      </div>
    </section>
  );
}

function Section({ title, children, onDropField }: { title: string; children: React.ReactNode; onDropField(f: FieldRef): void }) {
  const { t } = useT();
  const [over, setOver] = useState(false);
  return (
    <div className="filter-section" aria-label={title} role="group">
      <div className="filter-section-title">{title}</div>
      {children}
      <div
        className={`filter-drop ${over ? "is-over" : ""}`}
        onDragOver={(e) => {
          if (e.dataTransfer.types.includes(FIELD_MIME)) {
            e.preventDefault();
            setOver(true);
          }
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          const raw = e.dataTransfer.getData(FIELD_MIME);
          if (raw) onDropField(JSON.parse(raw) as FieldRef);
        }}
      >
        {t("Add data fields here")}
      </div>
    </div>
  );
}

function summary(f: ReportFilter, t: (s: string, ...a: (string | number)[]) => string): string {
  const k = f.filter;
  switch (k.kind) {
    case "all":
      return t("is (All)");
    case "basic": {
      const shown = k.values.slice(0, 2).map((v) => v ?? t("(Blank)"));
      const rest = k.values.length > 2 ? ` +${k.values.length - 2}` : "";
      return `${k.exclude ? t("is not") : t("is")} ${shown.join(", ")}${rest}`;
    }
    case "advanced":
      return k.conditions.map((c) => `${opLabel(c.op, t)} ${c.value ?? ""}`.trim()).join(k.or ? ` ${t("or")} ` : ` ${t("and")} `);
    case "topN":
      return k.bottom ? t("Bottom %1", k.count) : t("Top %1", k.count);
    case "relativeDate":
      return relativeLabel(k.period, k.count, k.unit, t);
  }
}

function FilterCard({ model, filter, scope, onChange, onRemove }: { model: Model; filter: ReportFilter; scope: Scope; onChange(k: FilterKind): void; onRemove?: () => void }) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const col = columnOf(model, filter.field.field);
  const isMeasure = filter.field.field.kind === "measure" || !!filter.field.aggregation;
  const kinds: FilterKind["kind"][] = isMeasure ? ["advanced"] : scope === "visual" ? ["basic", "advanced", "topN"] : ["basic", "advanced"];
  const [kind, setKind] = useState<FilterKind["kind"]>(filter.filter.kind === "all" ? kinds[0]! : filter.filter.kind);
  const active = filter.filter.kind !== "all";
  const name = projectionName(model, filter.field, t);
  return (
    <div className={`filter-card ${active ? "is-active" : ""}`} data-filter={name}>
      <div className="filter-card-head">
        <button type="button" className="filter-card-toggle" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          <span className="filter-card-name">{name}</span>
          <span className="filter-card-summary">{summary(filter, t)}</span>
        </button>
        {active && (
          <button type="button" className="filter-icon" aria-label={t("Clear filter")} title={t("Clear filter")} onClick={() => onChange({ kind: "all" })}>
            <Eraser size={13} />
          </button>
        )}
        {onRemove && (
          <button type="button" className="filter-icon" aria-label={t("Remove filter")} title={t("Remove filter")} onClick={onRemove}>
            <X size={13} />
          </button>
        )}
        <ChevronDown size={13} className={`filter-caret ${open ? "" : "is-closed"}`} />
      </div>
      {open && (
        <div className="filter-card-body">
          {kinds.length > 1 && (
            <label className="field">
              <span className="field-label">{t("Filter type")}</span>
              <select className="select" value={kind} onChange={(e) => setKind(e.target.value as FilterKind["kind"])}>
                {kinds.map((k) => (
                  <option key={k} value={k}>
                    {k === "basic" ? t("Basic filtering") : k === "advanced" ? t("Advanced filtering") : t("Top N")}
                  </option>
                ))}
              </select>
            </label>
          )}
          {kind === "basic" && <BasicFilter model={model} field={filter.field} current={filter.filter} onChange={onChange} />}
          {kind === "advanced" && <AdvancedFilter type={isMeasure ? "Double" : (col?.dataType ?? "String")} current={filter.filter} onChange={onChange} />}
          {kind === "topN" && <TopNFilter model={model} current={filter.filter} onChange={onChange} />}
        </div>
      )}
    </div>
  );
}

function BasicFilter({ model, field, current, onChange }: { model: Model; field: Projection; current: FilterKind; onChange(k: FilterKind): void }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const [search, setSearch] = useState("");
  const [list, setList] = useState<DistinctValues | null>(null);
  const [error, setError] = useState<string | null>(null);
  const col = columnOf(model, field.field);
  useEffect(() => {
    let live = true;
    const timer = setTimeout(() => {
      dataJson<DistinctValues>({ type: "fieldValues", field: { ...field, aggregation: null }, filters: [], search: search || null, limit: 500 })
        .then((d) => live && (setList(d), setError(null)))
        .catch((e) => live && setError(errorMessage(e)));
    }, search ? 150 : 0);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [field, search]);
  const values = current.kind === "basic" ? current.values : [];
  const exclude = current.kind === "basic" && current.exclude;
  const checked = (k: string | null) => (current.kind === "basic" ? values.includes(k) !== exclude : false);
  const toggle = (k: string | null) => {
    const base = current.kind === "basic" && !exclude ? values : [];
    const next = base.includes(k) ? base.filter((v) => v !== k) : [...base, k];
    onChange(next.length ? { kind: "basic", values: next, exclude: false } : { kind: "all" });
  };
  return (
    <div className="basic-filter">
      <label className="pane-search">
        <Search size={12} />
        <input value={search} placeholder={t("Search")} aria-label={t("Search values")} onChange={(e) => setSearch(e.target.value)} />
        {!list && !error && <Spinner size={11} />}
      </label>
      {error ? (
        <p className="filter-error">{error}</p>
      ) : (
        <ul className="basic-list" role="listbox" aria-multiselectable aria-label={t("Values")}>
          {list?.values.map((v) => (
            <li key={v.key ?? "\u0000"}>
              <label className="check">
                <input type="checkbox" checked={checked(v.key)} onChange={() => toggle(v.key)} />
                <span className={v.key === null ? "is-blank" : ""}>{v.key === null ? t("(Blank)") : formatValue(v.value, col?.dataType ?? "String", col?.formatString, locale) || v.key}</span>
              </label>
            </li>
          ))}
        </ul>
      )}
      {list && list.total > list.values.length && <p className="pane-hint">{t("Showing %1 of %2 values. Search to find others.", list.values.length, list.total)}</p>}
    </div>
  );
}

function AdvancedFilter({ type, current, onChange }: { type: DataType; current: FilterKind; onChange(k: FilterKind): void }) {
  const { t } = useT();
  const ops = type === "String" ? TEXT_OPS : NUM_OPS;
  const init = current.kind === "advanced" ? current : { conditions: [] as Condition[], or: false };
  const [c1, setC1] = useState<Condition>(init.conditions[0] ?? { op: ops[0]!, value: "", value2: null });
  const [c2, setC2] = useState<Condition>(init.conditions[1] ?? { op: ops[0]!, value: "", value2: null });
  const [or, setOr] = useState(init.or);
  const needs = (c: Condition) => c.op !== "isBlank" && c.op !== "isNotBlank";
  const complete = (c: Condition) => !needs(c) || !!c.value?.trim();
  const row = (c: Condition, set: (c: Condition) => void, label: string) => (
    <div className="adv-row">
      <select className="select" aria-label={label} value={c.op} onChange={(e) => set({ ...c, op: e.target.value as FilterOp })}>
        {ops.map((o) => (
          <option key={o} value={o}>
            {opLabel(o, t)}
          </option>
        ))}
      </select>
      {needs(c) && <input className="input" aria-label={t("Value")} value={c.value ?? ""} onChange={(e) => set({ ...c, value: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />}
    </div>
  );
  return (
    <div className="adv-filter">
      <span className="field-label">{t("Show items when the value")}</span>
      {row(c1, setC1, t("First condition"))}
      <div className="adv-join" role="radiogroup" aria-label={t("Join")}>
        <label className="check">
          <input type="radio" checked={!or} onChange={() => setOr(false)} /> {t("And")}
        </label>
        <label className="check">
          <input type="radio" checked={or} onChange={() => setOr(true)} /> {t("Or")}
        </label>
      </div>
      {row(c2, setC2, t("Second condition"))}
      <Button
        size="sm"
        variant="primary"
        isDisabled={!complete(c1)}
        onPress={() => {
          const conditions = [c1, ...(complete(c2) && (!needs(c2) || c2.value) ? [c2] : [])].map((c) => ({ ...c, value: needs(c) ? (c.value ?? "").trim() : null }));
          onChange({ kind: "advanced", conditions, or });
        }}
      >
        {t("Apply filter")}
      </Button>
    </div>
  );
}

function TopNFilter({ model, current, onChange }: { model: Model; current: FilterKind; onChange(k: FilterKind): void }) {
  const { t } = useT();
  const init = current.kind === "topN" ? current : null;
  const [bottom, setBottom] = useState(init?.bottom ?? false);
  const [count, setCount] = useState(init?.count ?? 5);
  const measures = model.tables.flatMap((tb) => tb.measures.filter((m) => !m.hidden).map((m) => ({ table: tb.id, measure: m.id, name: m.name })));
  const [by, setBy] = useState(init?.by.field.kind === "measure" ? init.by.field.measure : (measures[0]?.measure ?? ""));
  const m = measures.find((x) => x.measure === by);
  return (
    <div className="adv-filter">
      <span className="field-label">{t("Show items")}</span>
      <div className="adv-row">
        <select className="select" aria-label={t("Top or bottom")} value={bottom ? "bottom" : "top"} onChange={(e) => setBottom(e.target.value === "bottom")}>
          <option value="top">{t("Top")}</option>
          <option value="bottom">{t("Bottom")}</option>
        </select>
        <input className="input" type="number" min={1} aria-label={t("Count")} value={count} onChange={(e) => setCount(Math.max(1, Number(e.target.value) || 1))} onKeyDown={(e) => e.stopPropagation()} />
      </div>
      <label className="field">
        <span className="field-label">{t("By value")}</span>
        <select className="select" value={by} onChange={(e) => setBy(e.target.value)}>
          {measures.map((x) => (
            <option key={x.measure} value={x.measure}>
              {x.name}
            </option>
          ))}
        </select>
      </label>
      <Button
        size="sm"
        variant="primary"
        isDisabled={!m}
        onPress={() => m && onChange({ kind: "topN", count, bottom, by: { field: { kind: "measure", table: m.table, measure: m.measure }, aggregation: null, displayName: null } })}
      >
        {t("Apply filter")}
      </Button>
    </div>
  );
}

