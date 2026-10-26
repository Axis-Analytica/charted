// The format pane's conditional formatting ("Cell elements") for tables and matrices:
// each value column with its background colour, font colour, data bars and icons, and
// the dialog that edits one of them, like Power BI's: format by a gradient or by rules,
// based on the column's own values or on another field (a measure need not be in the
// visual).
import { Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { Visual } from "../bridge/generated/Visual";
import { msg, useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { defaultAggregation, editVisual, fieldKey, isGrouping, projectionName } from "../state/report";
import { CF_ICONS } from "./visuals/CfCell";
import { type CfBound, type CfIcon, type CfKind, type CfRule, type CfRuleLine, CF_KINDS, cfKey, columnFormatting, defaultRule } from "./visuals/conditional";
import { palette, token } from "./visuals/common";

const KIND_LABEL: Record<CfKind, string> = { background: msg("Background color"), fontColor: msg("Font color"), dataBars: msg("Data bars"), icons: msg("Icons") };

/** Any CSS colour as #rrggbb (colour inputs need it). */
function hex(color: string, fallback = "#4e79a7"): string {
  if (/^#[0-9a-f]{6}$/i.test(color)) return color;
  const c = document.createElement("canvas").getContext("2d");
  if (!c || !color) return fallback;
  c.fillStyle = fallback;
  c.fillStyle = color;
  const v = c.fillStyle;
  if (/^#[0-9a-f]{6}$/i.test(v)) return v;
  const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(v);
  return m ? `#${[m[1], m[2], m[3]].map((x) => Number(x).toString(16).padStart(2, "0")).join("")}` : fallback;
}

/** The columns conditional formatting applies to: a table's values, a matrix's values. */
function formattable(visual: Visual): Projection[] {
  if (visual.visualType === "matrix") return visual.roles.values ?? [];
  return (visual.roles.columns ?? []).filter((p) => !isGrouping(p));
}

export function ConditionalFormattingSection({ model, page, visual }: { model: Model; page: Page; visual: Visual }) {
  const { t } = useT();
  const [editing, setEditing] = useState<{ column: Projection; kind: CfKind } | null>(null);
  const columns = formattable(visual);
  return (
    <div className="cf-list">
      {!columns.length && <p className="pane-hint">{t("Add a value to format its cells.")}</p>}
      {columns.map((p) => {
        const cf = columnFormatting(visual, p);
        const name = projectionName(model, p, t);
        return (
          <div key={cfKey(p)} className="cf-column" data-cf-column={name}>
            <div className="cf-row-name" title={name}>
              {name}
            </div>
            <div className="cf-kinds" role="group" aria-label={t("Conditional formatting for %1", name)}>
              {CF_KINDS.map((k) => (
                <button key={k} type="button" className={`cf-kind ${cf[k] ? "is-on" : ""}`} aria-pressed={!!cf[k]} onClick={() => setEditing({ column: p, kind: k })}>
                  {t(KIND_LABEL[k])}
                </button>
              ))}
            </div>
          </div>
        );
      })}
      {editing && <CfDialog key={`${cfKey(editing.column)}${editing.kind}`} model={model} page={page} visual={visual} column={editing.column} kind={editing.kind} onClose={() => setEditing(null)} />}
    </div>
  );
}

/** Measures and numeric columns (summed) a rule can be based on. */
function baseFields(model: Model): { key: string; label: string; p: Projection }[] {
  const out: { key: string; label: string; p: Projection }[] = [];
  for (const tb of model.tables) {
    for (const m of tb.measures.filter((x) => !x.hidden)) {
      const p: Projection = { field: { kind: "measure", table: tb.id, measure: m.id }, aggregation: null, displayName: null };
      out.push({ key: `${fieldKey(p.field)}|`, label: `${tb.name}[${m.name}]`, p });
    }
    for (const c of tb.columns.filter((x) => !x.hidden && (x.dataType === "Int64" || x.dataType === "Double" || x.dataType === "Decimal"))) {
      const agg = defaultAggregation(c);
      const p: Projection = { field: { kind: "column", table: tb.id, column: c.id }, aggregation: agg, displayName: null };
      out.push({ key: `${fieldKey(p.field)}|${agg}`, label: `${tb.name}[${c.name}] (${agg})`, p });
    }
  }
  return out;
}

function CfDialog({ model, page, visual, column, kind, onClose }: { model: Model; page: Page; visual: Visual; column: Projection; kind: CfKind; onClose(): void }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const existing = columnFormatting(visual, column)[kind];
  const accent = hex(palette()[0] ?? token("--accent"));
  const [rule, setRule] = useState<CfRule>(() => existing ?? defaultRule(kind, accent, hex(token("--success"), "#3a9a5b"), hex(token("--danger"), "#d0453a"), hex(token("--warning"), "#e0a030")));
  const fields = baseFields(model);
  const name = projectionName(model, column, t);
  const save = (next: CfRule | null) => {
    const key = cfKey(column);
    edit(
      "Conditional formatting",
      editVisual(page.id, visual.id, (v) => {
        const all = { ...((v.objects?.conditionalFormatting ?? {}) as Record<string, Record<string, unknown>>) };
        const col = { ...(all[key] ?? {}) };
        if (next) col[kind] = next;
        else delete col[kind];
        if (Object.keys(col).length) all[key] = col;
        else delete all[key];
        return { ...v, objects: { ...(v.objects ?? {}), conditionalFormatting: all } };
      }),
    );
    onClose();
  };
  const set = (patch: Partial<CfRule>) => setRule((r) => ({ ...r, ...patch }));
  const boundRow = (label: string, b: CfBound, color: string, onBound: (b: CfBound) => void, onColor: ((c: string) => void) | null, auto: "lowest" | "highest" | null) => (
    <div className="field">
      <span className="field-label">{label}</span>
      <div className="adv-row">
        <select className="select" aria-label={t("%1 type", label)} value={b.kind} onChange={(e) => onBound({ ...b, kind: e.target.value as CfBound["kind"] })}>
          {auto && <option value={auto}>{auto === "lowest" ? t("Lowest value") : t("Highest value")}</option>}
          <option value="number">{t("Number")}</option>
        </select>
        {b.kind === "number" && <input className="input" type="number" aria-label={t("%1 value", label)} value={b.value} onChange={(e) => onBound({ ...b, value: Number(e.target.value) })} onKeyDown={(e) => e.stopPropagation()} />}
        {onColor && <input type="color" aria-label={t("%1 color", label)} value={hex(color)} onChange={(e) => onColor(e.target.value)} />}
      </div>
    </div>
  );
  const ruleLine = (line: CfRuleLine, i: number) => {
    const change = (patch: Partial<CfRuleLine>) => set({ rules: rule.rules.map((x, j) => (j === i ? { ...x, ...patch } : x)) });
    const num = (v: string) => (v.trim() === "" ? null : Number(v));
    return (
      <div key={i} className="cf-rule" data-cf-rule={i}>
        <span>{t("If value ≥")}</span>
        <input className="input" type="number" aria-label={t("Rule %1 from", i + 1)} value={line.from ?? ""} placeholder={t("Any")} onChange={(e) => change({ from: num(e.target.value) })} onKeyDown={(e) => e.stopPropagation()} />
        <span>{t("and <")}</span>
        <input className="input" type="number" aria-label={t("Rule %1 to", i + 1)} value={line.to ?? ""} placeholder={t("Any")} onChange={(e) => change({ to: num(e.target.value) })} onKeyDown={(e) => e.stopPropagation()} />
        <select className="select" aria-label={t("Rule %1 unit", i + 1)} value={line.unit} onChange={(e) => change({ unit: e.target.value as CfRuleLine["unit"] })}>
          <option value="number">{t("Number")}</option>
          <option value="percent">{t("Percent")}</option>
        </select>
        <span className="cf-rule-then">
          {kind === "icons" && (
            <select className="select" aria-label={t("Rule %1 icon", i + 1)} value={line.icon ?? "up"} onChange={(e) => change({ icon: e.target.value as CfIcon })}>
              {(Object.keys(CF_ICONS) as CfIcon[]).map((ic) => (
                <option key={ic} value={ic}>
                  {t(ICON_LABEL[ic])}
                </option>
              ))}
            </select>
          )}
          <input type="color" aria-label={t("Rule %1 color", i + 1)} value={hex(line.color)} onChange={(e) => change({ color: e.target.value })} />
          <button type="button" className="filter-icon" aria-label={t("Remove rule %1", i + 1)} onClick={() => set({ rules: rule.rules.filter((_, j) => j !== i) })}>
            <Trash2 size={13} />
          </button>
        </span>
      </div>
    );
  };
  const fieldValue = rule.field ? `${fieldKey(rule.field.field)}|${rule.field.aggregation ?? ""}` : "";
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("%1 – %2", t(KIND_LABEL[kind]), name)}
      width={560}
      footer={
        <>
          {existing && (
            <Button variant="danger" onPress={() => save(null)}>
              {t("Turn off")}
            </Button>
          )}
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" onPress={() => save(rule)}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="cf-form">
        {kind !== "dataBars" && kind !== "icons" && (
          <label className="field">
            <span className="field-label">{t("Format style")}</span>
            <select className="select" value={rule.mode} onChange={(e) => set({ mode: e.target.value as CfRule["mode"], rules: rule.rules.length ? rule.rules : [{ from: null, to: null, unit: "number", color: hex(rule.maxColor) }] })}>
              <option value="gradient">{t("Gradient")}</option>
              <option value="rules">{t("Rules")}</option>
            </select>
          </label>
        )}
        <label className="field">
          <span className="field-label">{t("What field should we base this on?")}</span>
          <select className="select" value={fieldValue} onChange={(e) => set({ field: fields.find((f) => f.key === e.target.value)?.p ?? null })}>
            <option value="">{t("This column's values")}</option>
            {fields.map((f) => (
              <option key={f.key} value={f.key}>
                {f.label}
              </option>
            ))}
          </select>
        </label>
        {kind === "dataBars" ? (
          <>
            <div className="cf-grid">
              {boundRow(t("Minimum"), rule.min, "", (b) => set({ min: b }), null, "lowest")}
              {boundRow(t("Maximum"), rule.max, "", (b) => set({ max: b }), null, "highest")}
            </div>
            <div className="adv-row">
              <label className="field">
                <span className="field-label">{t("Positive bar")}</span>
                <input type="color" aria-label={t("Positive bar")} value={hex(rule.positive ?? accent)} onChange={(e) => set({ positive: e.target.value })} />
              </label>
              <label className="field">
                <span className="field-label">{t("Negative bar")}</span>
                <input type="color" aria-label={t("Negative bar")} value={hex(rule.negative ?? "#d0453a")} onChange={(e) => set({ negative: e.target.value })} />
              </label>
            </div>
            <label className="check">
              <input type="checkbox" checked={!!rule.barOnly} onChange={(e) => set({ barOnly: e.target.checked })} />
              {t("Show bar only")}
            </label>
          </>
        ) : rule.mode === "gradient" && kind !== "icons" ? (
          <>
            <div className="cf-grid">
              {boundRow(t("Minimum"), rule.min, rule.minColor, (b) => set({ min: b }), (c) => set({ minColor: c }), "lowest")}
              {rule.mid && boundRow(t("Center"), rule.mid, rule.midColor, (b) => set({ mid: b }), (c) => set({ midColor: c }), null)}
              {boundRow(t("Maximum"), rule.max, rule.maxColor, (b) => set({ max: b }), (c) => set({ maxColor: c }), "highest")}
            </div>
            <label className="check">
              <input type="checkbox" checked={!!rule.mid} onChange={(e) => set({ mid: e.target.checked ? { kind: "number", value: 0 } : null })} />
              {t("Add a middle color")}
            </label>
          </>
        ) : (
          <div className="cf-rules">
            {rule.rules.map(ruleLine)}
            <Button size="sm" variant="ghost" onPress={() => set({ rules: [...rule.rules, { from: null, to: null, unit: "number", color: accent, icon: kind === "icons" ? "flag" : undefined }] })}>
              <Plus size={13} /> {t("New rule")}
            </Button>
          </div>
        )}
      </div>
    </Dialog>
  );
}

const ICON_LABEL: Record<CfIcon, string> = {
  up: msg("Arrow up"),
  right: msg("Arrow right"),
  down: msg("Arrow down"),
  good: msg("Check"),
  warn: msg("Warning"),
  bad: msg("Cross"),
  flag: msg("Flag"),
  star: msg("Star"),
};
