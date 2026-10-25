// A dialog for adding or editing one step, generated from its transform's fields.
import { Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { errorMessage } from "../bridge";
import type { Aggregate } from "../bridge/generated/Aggregate";
import type { Condition } from "../bridge/generated/Condition";
import type { Query } from "../bridge/generated/Query";
import type { Rule } from "../bridge/generated/Rule";
import type { Step } from "../bridge/generated/Step";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { queryInfo } from "./data";
import { AGGS, type BuildContext, type Field, missing, OPS, type Transform, type Values } from "./transforms";

interface Props {
  transform: Transform;
  /** The step being edited (its gear), or null for a new one. */
  step: Step | null;
  /** The column the transform was started from. */
  column: string | null;
  ctx: BuildContext;
  onDone(step: Step | null): void;
}

function ColumnPicker({ columns, value, onChange, label }: { columns: string[]; value: string | null; onChange(v: string): void; label: string }) {
  const { t } = useT();
  return (
    <select className="select" aria-label={label} value={value ?? ""} onChange={(e) => onChange(e.target.value)}>
      <option value="" disabled>
        {t("Choose a column")}
      </option>
      {columns.map((c) => (
        <option key={c} value={c}>
          {c}
        </option>
      ))}
    </select>
  );
}

function ColumnsPicker({ columns, value, onChange, label }: { columns: string[]; value: string[]; onChange(v: string[]): void; label: string }) {
  const { t } = useT();
  return (
    <div className="qe-columns" role="group" aria-label={label}>
      <div className="qe-columns-tools">
        <button type="button" className="link-btn" onClick={() => onChange(columns)}>
          {t("Select all")}
        </button>
        <button type="button" className="link-btn" onClick={() => onChange([])}>
          {t("Clear")}
        </button>
      </div>
      <div className="qe-columns-list">
        {columns.map((c) => (
          <label key={c} className="check">
            <input
              type="checkbox"
              checked={value.includes(c)}
              onChange={(e) => onChange(e.target.checked ? [...value, c].sort((a, b) => columns.indexOf(a) - columns.indexOf(b)) : value.filter((x) => x !== c))}
            />
            {c}
          </label>
        ))}
      </div>
    </div>
  );
}

const needsValue = (op: Condition["op"]) => op !== "isBlank" && op !== "isNotBlank";

export function StepDialog({ transform, step, column, ctx, onDone }: Props) {
  const { t } = useT();
  const [v, setV] = useState<Values>(() => transform.init(step, column));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [other, setOther] = useState<Record<string, string[]>>({});
  const set = (k: string, x: unknown) => setV((o) => ({ ...o, [k]: x }));

  // Columns of the query chosen to merge with.
  const otherId = typeof v.query === "string" ? v.query : null;
  useEffect(() => {
    if (!otherId || other[otherId]) return;
    const q = ctx.queries.find((x) => x.id === otherId);
    if (!q) return;
    queryInfo(q, ctx.queries, q.steps.length)
      .then((info) => setOther((o) => ({ ...o, [otherId]: info.columns.map((c) => c.name) })))
      .catch((e) => setError(errorMessage(e)));
  }, [otherId, other, ctx.queries]);

  const save = async () => {
    const gap = missing(transform, v);
    if (gap) return setError(t("Fill in “%1”.", t(gap)));
    setBusy(true);
    setError(null);
    try {
      onDone(await transform.build(v, ctx));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const field = (f: Field) => {
    const label = t(f.label);
    switch (f.type) {
      case "column":
        return <ColumnPicker columns={ctx.columns} value={(v[f.key] as string) ?? null} onChange={(x) => set(f.key, x)} label={label} />;
      case "columns":
        return <ColumnsPicker columns={ctx.columns} value={(v[f.key] as string[]) ?? []} onChange={(x) => set(f.key, x)} label={label} />;
      case "otherColumns":
        return <ColumnsPicker columns={otherId ? (other[otherId] ?? []) : []} value={(v[f.key] as string[]) ?? []} onChange={(x) => set(f.key, x)} label={label} />;
      case "text":
        return <input className="input" aria-label={label} value={(v[f.key] as string) ?? ""} placeholder={f.placeholder ? t(f.placeholder) : undefined} onChange={(e) => set(f.key, e.target.value)} onKeyDown={(e) => e.stopPropagation()} />;
      case "number":
        return <input className="input" type="number" aria-label={label} min={f.min} value={String(v[f.key] ?? "")} onChange={(e) => set(f.key, e.target.value === "" ? "" : Number(e.target.value))} onKeyDown={(e) => e.stopPropagation()} />;
      case "select":
        return (
          <select className="select" aria-label={label} value={String(v[f.key] ?? "")} onChange={(e) => set(f.key, e.target.value)}>
            {f.options.map((o) => (
              <option key={o.value} value={o.value}>
                {t(o.label)}
              </option>
            ))}
          </select>
        );
      case "bool":
        return (
          <label className="check">
            <input type="checkbox" checked={!!v[f.key]} onChange={(e) => set(f.key, e.target.checked)} />
            {label}
          </label>
        );
      case "query": {
        const others = ctx.queries.filter((q) => q.id !== ctx.query.id);
        if (f.multi) {
          const chosen = (v[f.key] as string[]) ?? [];
          return (
            <div className="qe-columns-list" role="group" aria-label={label}>
              {others.map((q) => (
                <label key={q.id} className="check">
                  <input type="checkbox" checked={chosen.includes(q.id)} onChange={(e) => set(f.key, e.target.checked ? [...chosen, q.id] : chosen.filter((x) => x !== q.id))} />
                  {q.name}
                </label>
              ))}
            </div>
          );
        }
        return (
          <select className="select" aria-label={label} value={(v[f.key] as string) ?? ""} onChange={(e) => (set(f.key, e.target.value), set("right", []), set("columns", []))}>
            <option value="" disabled>
              {t("Choose a query")}
            </option>
            {others.map((q) => (
              <option key={q.id} value={q.id}>
                {q.name}
              </option>
            ))}
          </select>
        );
      }
      case "formula":
        return (
          <div className="qe-formula">
            <textarea className="input mono" rows={4} aria-label={label} value={(v[f.key] as string) ?? ""} onChange={(e) => set(f.key, e.target.value)} onKeyDown={(e) => e.stopPropagation()} spellCheck={false} />
            <div className="qe-formula-help">
              <span>{t("Available columns (click to insert):")}</span>
              <div className="qe-chips">
                {ctx.columns.map((c) => (
                  <button key={c} type="button" className="qe-chip" onClick={() => set(f.key, `${(v[f.key] as string) ?? ""}[${c}]`)}>
                    {c}
                  </button>
                ))}
              </div>
              <span className="qe-hint">{t("Like Power Query: [Quantity] * [Price], if [Region] = \"EU\" then \"Europe\" else [Region], Text.Upper([Name]).")}</span>
            </div>
          </div>
        );
      case "conditions": {
        const conds = (v[f.key] as Condition[]) ?? [];
        const setCond = (i: number, c: Partial<Condition>) => set(f.key, conds.map((x, j) => (j === i ? { ...x, ...c } : x)));
        return (
          <div className="qe-rules">
            {conds.map((c, i) => (
              <div key={i} className="qe-rule">
                {i > 0 && (
                  <select className="select qe-andor" aria-label={t("And or or")} value={v.or ? "or" : "and"} onChange={(e) => set("or", e.target.value === "or")}>
                    <option value="and">{t("and")}</option>
                    <option value="or">{t("or")}</option>
                  </select>
                )}
                <select className="select" aria-label={t("Condition")} value={c.op} onChange={(e) => setCond(i, { op: e.target.value as Condition["op"] })}>
                  {OPS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {t(o.label)}
                    </option>
                  ))}
                </select>
                {needsValue(c.op) && <input className="input" aria-label={t("Value")} value={c.value ?? ""} onChange={(e) => setCond(i, { value: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />}
                {conds.length > 1 && <IconButton label={t("Remove condition")} icon={<Trash2 size={13} />} onPress={() => set(f.key, conds.filter((_, j) => j !== i))} />}
              </div>
            ))}
            {conds.length < 2 && (
              <Button size="sm" variant="ghost" icon={<Plus size={13} />} onPress={() => set(f.key, [...conds, { op: "equals", value: "", value2: null }])}>
                {t("Add condition")}
              </Button>
            )}
          </div>
        );
      }
      case "rules": {
        const rules = (v[f.key] as Rule[]) ?? [];
        const setRule = (i: number, r: Partial<Rule>) => set(f.key, rules.map((x, j) => (j === i ? { ...x, ...r } : x)));
        return (
          <div className="qe-rules">
            {rules.map((r, i) => (
              <div key={i} className="qe-rule qe-rule-wide">
                <span className="qe-rule-word">{i === 0 ? t("If") : t("Else if")}</span>
                <ColumnPicker columns={ctx.columns} value={r.column || null} onChange={(x) => setRule(i, { column: x })} label={t("Column")} />
                <select className="select" aria-label={t("Condition")} value={r.op} onChange={(e) => setRule(i, { op: e.target.value as Rule["op"] })}>
                  {OPS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {t(o.label)}
                    </option>
                  ))}
                </select>
                {needsValue(r.op) && <input className="input" aria-label={t("Value")} value={r.value ?? ""} onChange={(e) => setRule(i, { value: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />}
                <span className="qe-rule-word">{t("Then")}</span>
                <input className="input" aria-label={t("Output")} value={r.output} onChange={(e) => setRule(i, { output: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
                {rules.length > 1 && <IconButton label={t("Remove rule")} icon={<Trash2 size={13} />} onPress={() => set(f.key, rules.filter((_, j) => j !== i))} />}
              </div>
            ))}
            <Button size="sm" variant="ghost" icon={<Plus size={13} />} onPress={() => set(f.key, [...rules, { column: rules.at(-1)?.column ?? "", op: "equals", value: "", output: "" }])}>
              {t("Add clause")}
            </Button>
          </div>
        );
      }
      case "aggregates": {
        const aggs = (v[f.key] as Aggregate[]) ?? [];
        const setAgg = (i: number, a: Partial<Aggregate>) => set(f.key, aggs.map((x, j) => (j === i ? { ...x, ...a } : x)));
        return (
          <div className="qe-rules">
            {aggs.map((a, i) => (
              <div key={i} className="qe-rule">
                <input className="input" aria-label={t("New column name")} value={a.name} onChange={(e) => setAgg(i, { name: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
                <select className="select" aria-label={t("Operation")} value={a.op} onChange={(e) => setAgg(i, { op: e.target.value as Aggregate["op"] })}>
                  {AGGS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {t(o.label)}
                    </option>
                  ))}
                </select>
                {a.op !== "countRows" && <ColumnPicker columns={ctx.columns} value={a.column} onChange={(x) => setAgg(i, { column: x })} label={t("Column")} />}
                {aggs.length > 1 && <IconButton label={t("Remove aggregation")} icon={<Trash2 size={13} />} onPress={() => set(f.key, aggs.filter((_, j) => j !== i))} />}
              </div>
            ))}
            <Button size="sm" variant="ghost" icon={<Plus size={13} />} onPress={() => set(f.key, [...aggs, { name: t("Sum"), op: "sum", column: null }])}>
              {t("Add aggregation")}
            </Button>
          </div>
        );
      }
    }
  };

  // Extract shows only the fields its kind uses.
  const shown = transform.fields.filter((f) => {
    if (transform.id !== "extractText") return true;
    const k = v.kind;
    if (f.key === "count") return k === "first" || k === "last" || k === "range";
    if (f.key === "start") return k === "range";
    if (f.key === "delimiter") return k === "beforeDelimiter" || k === "afterDelimiter";
    return true;
  });

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone(null)}
      title={t(transform.label)}
      width={transform.fields.some((f) => f.type === "rules") ? 760 : 520}
      footer={
        <>
          <Button variant="ghost" onPress={() => onDone(null)}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy} onPress={save}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <form
        className="qe-form"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        {shown.map((f) => (
          <div key={f.key} className="field">
            {f.type !== "bool" && <span className="field-label">{t(f.label)}</span>}
            {field(f)}
          </div>
        ))}
        {error && (
          <p className="qe-error" role="alert">
            {error}
          </p>
        )}
      </form>
    </Dialog>
  );
}

export type { Query };
