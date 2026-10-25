// The formula bar: edit a measure as `Name = expression`, checked against the model as
// you type (parse, names, supported functions, cycles). Enter commits, Shift+Enter
// adds a line, Escape cancels. Quick measures write the DAX for common calculations.
import { Check, Sparkles, Trash2, WandSparkles, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DaxCheck } from "../bridge/generated/DaxCheck";
import type { Measure } from "../bridge/generated/Measure";
import type { Model } from "../bridge/generated/Model";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";
import { newId, useReport } from "../state/report";
import { DaxEditor, type DaxEditorHandle } from "../dax/LazyDaxEditor";
import { splitFormula } from "../dax/service";

const FORMATS = [
  { format: "", label: "General" },
  { format: "#,0", label: "Whole number" },
  { format: "#,0.00", label: "Decimal number" },
  { format: "\\$#,0;(\\$#,0)", label: "Currency" },
  { format: "\\$#,0.00;(\\$#,0.00)", label: "Currency (cents)" },
  { format: "0.0%", label: "Percentage" },
];

/** `Name = expression` → its parts; a missing name gets `fallback`. `lead` is where the
 * trimmed expression starts after `exprStart`. */
function split(text: string, fallback: string): { name: string; expression: string; exprStart: number; lead: number } {
  const { name, exprStart } = splitFormula(text);
  const rest = text.slice(exprStart);
  return { name: name ?? fallback, expression: rest.trim(), exprStart, lead: rest.length - rest.trimStart().length };
}

function uniqueName(model: Model, base: string): string {
  const taken = new Set(model.tables.flatMap((t) => [...t.measures.map((m) => m.name.toLowerCase()), ...t.columns.map((c) => c.name.toLowerCase())]));
  if (!taken.has(base.toLowerCase())) return base;
  for (let i = 2; ; i++) if (!taken.has(`${base} ${i}`.toLowerCase())) return `${base} ${i}`;
}

export function FormulaBar({ model }: { model: Model }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const formula = useReport((s) => s.formula);
  const set = useReport((s) => s.set);
  const ref = useRef<DaxEditorHandle>(null);
  const isNew = formula?.startsWith("new:") ?? false;
  const homeId = isNew ? formula!.slice(4) : model.tables.find((tb) => tb.measures.some((m) => m.id === formula))?.id;
  const home = model.tables.find((tb) => tb.id === homeId);
  const measure: Measure | undefined = isNew ? undefined : home?.measures.find((m) => m.id === formula);
  const fallback = useMemo(() => uniqueName(model, t("Measure")), [model, t]);
  const initial = measure ? `${measure.name} = ${measure.expression}` : `${fallback} = `;
  const [text, setText] = useState(initial);
  const [check, setCheck] = useState<DaxCheck | null>(null);
  const [quick, setQuick] = useState(false);

  // A different measure (or an undo that changed this one) resets the text.
  useEffect(() => {
    setText(initial);
    setCheck(null);
    requestAnimationFrame(() => {
      if (isNew) ref.current?.focus();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [formula, measure?.expression, measure?.name]);

  const { name, expression, exprStart, lead } = split(text, fallback);
  // Diagnostics are positions in the expression; show them as positions in the text.
  const exprOffset = exprStart + lead;
  useEffect(() => {
    if (!formula || !expression) {
      setCheck(null);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      dataJson<DaxCheck>({ type: "checkDax", expression, measure: measure?.id ?? null })
        .then((c) => live && setCheck(c))
        .catch(() => {});
    }, 200);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [formula, expression, measure?.id]);

  if (!formula || !home) return null;
  const problem = check?.diagnostics[0];
  const dirty = text !== initial;

  const save = async () => {
    if (!expression) return;
    if (measure) {
      if (!dirty) return;
      await commit("Edit measure", [{ op: "updateMeasure", table: home.id, measure: { ...measure, name, expression } }]);
      return;
    }
    const id = newId();
    const m: Measure = { id, name, expression, language: "Dax", description: null, formatString: null, displayFolder: null, hidden: false };
    if (await commit("New measure", [{ op: "addMeasure", table: home.id, measure: m, index: null }])) set({ formula: id, field: { kind: "measure", table: home.id, measure: id } });
  };

  return (
    <div className={`formula-bar ${problem ? "has-error" : ""}`} role="group" aria-label={t("Formula bar")}>
      <IconButton label={t("Cancel")} shortcut="Escape" icon={<X size={15} />} onPress={() => (isNew ? set({ formula: null }) : setText(initial))} />
      <IconButton label={t("Commit")} shortcut="Enter" icon={<Check size={15} />} isDisabled={!expression || (!isNew && !dirty)} onPress={save} />
      <div className="formula-edit">
        <DaxEditor
          ref={ref}
          label={t("Measure formula")}
          model={model}
          value={text}
          onChange={setText}
          onCommit={save}
          onCancel={() => (isNew ? set({ formula: null }) : setText(initial))}
          extra={problem ? { message: problem.message, start: lead + problem.span.start, end: lead + problem.span.end } : null}
        />
        {problem && (
          <div className="formula-problem" role="alert">
            {t("Column %1:", exprOffset + problem.span.start + 1)} {problem.message}
          </div>
        )}
      </div>
      <div className="formula-tools">
        {measure && (
          <select
            className="select"
            aria-label={t("Format")}
            value={FORMATS.some((f) => f.format === (measure.formatString ?? "")) ? (measure.formatString ?? "") : "custom"}
            onChange={(e) => e.target.value !== "custom" && commit("Change format", [{ op: "updateMeasure", table: home.id, measure: { ...measure, formatString: e.target.value || null } }])}
          >
            {FORMATS.map((f) => (
              <option key={f.format} value={f.format}>
                {t(f.label)}
              </option>
            ))}
            {!FORMATS.some((f) => f.format === (measure.formatString ?? "")) && <option value="custom">{measure.formatString}</option>}
          </select>
        )}
        <IconButton label={t("Format DAX")} shortcut="Shift+Alt+F" icon={<WandSparkles size={15} />} onPress={() => ref.current?.format()} />
        <IconButton label={t("Quick measure")} icon={<Sparkles size={15} />} onPress={() => setQuick(true)} />
        {measure && (
          <IconButton
            label={t("Delete measure")}
            icon={<Trash2 size={15} />}
            onPress={async () => {
              if (await commit("Delete measure", [{ op: "removeMeasure", table: home.id, measure: measure.id }])) set({ formula: null, field: null });
            }}
          />
        )}
        <IconButton label={t("Close formula bar")} icon={<X size={15} />} onPress={() => set({ formula: null })} />
      </div>
      <QuickMeasure
        open={quick}
        model={model}
        home={home.id}
        onClose={() => setQuick(false)}
        onCreate={(n, e) => {
          setQuick(false);
          setText(`${uniqueName(model, n)} = ${e}`);
          set({ formula: `new:${home.id}` });
          requestAnimationFrame(() => ref.current?.focus());
        }}
      />
    </div>
  );
}

type Calc = "sum" | "average" | "countRows" | "distinct" | "ratio" | "difference" | "percentDifference";

function QuickMeasure({ open, model, home, onClose, onCreate }: { open: boolean; model: Model; home: string; onClose(): void; onCreate(name: string, expression: string): void }) {
  const { t } = useT();
  const [calc, setCalc] = useState<Calc>("ratio");
  // Fields as DAX references.
  const refs = useMemo(() => {
    const quote = (s: string) => (/^[A-Za-z_][A-Za-z0-9_]*$/.test(s) ? s : `'${s.replace(/'/g, "''")}'`);
    const out: { ref: string; label: string; short: string; kind: "measure" | "column"; numeric: boolean; table: string }[] = [];
    for (const tb of model.tables) {
      for (const m of tb.measures) out.push({ ref: `[${m.name}]`, label: m.name, short: m.name, kind: "measure", numeric: true, table: tb.name });
      for (const c of tb.columns) {
        const numeric = c.dataType === "Int64" || c.dataType === "Double" || c.dataType === "Decimal";
        out.push({ ref: `${quote(tb.name)}[${c.name}]`, label: `${tb.name}[${c.name}]`, short: c.name, kind: "column", numeric, table: tb.name });
      }
    }
    return out;
  }, [model]);
  const tables = model.tables.map((tb) => tb.name);
  const values = refs.filter((r) => r.kind === "measure" || r.numeric);
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [table, setTable] = useState("");
  const first = (s: string) => s || values[0]?.ref || "";
  const val = (r: string) => (refs.find((x) => x.ref === r)?.kind === "column" ? `SUM ( ${r} )` : r);
  const label = (r: string) => refs.find((x) => x.ref === r)?.short ?? r;
  const A = first(a);
  const B = b || values[1]?.ref || A;
  const T = table || model.tables.find((x) => x.id === home)?.name || tables[0] || "";
  const quoteT = /^[A-Za-z_][A-Za-z0-9_]*$/.test(T) ? T : `'${T.replace(/'/g, "''")}'`;
  const col = refs.find((x) => x.ref === A && x.kind === "column");
  const result: [string, string] = (() => {
    switch (calc) {
      case "sum":
        return [`Total ${label(A)}`, val(A)];
      case "average":
        return col ? [`Average ${label(A)}`, `AVERAGE ( ${A} )`] : [`Average ${label(A)}`, `${A}`];
      case "countRows":
        return [`${T} rows`, `COUNTROWS ( ${quoteT} )`];
      case "distinct":
        return [`Distinct ${label(A)}`, col ? `DISTINCTCOUNT ( ${A} )` : `DISTINCTCOUNT ( ${A} )`];
      case "ratio":
        return [`${label(A)} per ${label(B)}`, `DIVIDE ( ${val(A)}, ${val(B)} )`];
      case "difference":
        return [`${label(A)} - ${label(B)}`, `${val(A)} - ${val(B)}`];
      case "percentDifference":
        return [`${label(A)} vs ${label(B)} %`, `DIVIDE ( ${val(A)} - ${val(B)}, ${val(B)} )`];
    }
  })();
  const pickA = calc !== "countRows";
  const pickB = calc === "ratio" || calc === "difference" || calc === "percentDifference";
  const options = calc === "distinct" ? refs.filter((r) => r.kind === "column") : values;
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && onClose()}
      title={t("Quick measure")}
      width={480}
      footer={
        <>
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" onPress={() => onCreate(result[0].replace(/[[\]]/g, ""), result[1])}>
            {t("Add")}
          </Button>
        </>
      }
    >
      <div className="quick-measure">
        <label className="field">
          <span className="field-label">{t("Calculation")}</span>
          <select className="select" value={calc} onChange={(e) => setCalc(e.target.value as Calc)}>
            <option value="sum">{t("Total")}</option>
            <option value="average">{t("Average")}</option>
            <option value="countRows">{t("Count of rows")}</option>
            <option value="distinct">{t("Count (distinct)")}</option>
            <option value="ratio">{t("Ratio (divide)")}</option>
            <option value="difference">{t("Difference")}</option>
            <option value="percentDifference">{t("Percentage difference")}</option>
          </select>
        </label>
        {pickA && (
          <label className="field">
            <span className="field-label">{pickB ? t("First value") : t("Value")}</span>
            <select className="select" value={A} onChange={(e) => setA(e.target.value)}>
              {options.map((r) => (
                <option key={r.ref} value={r.ref}>
                  {r.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {pickB && (
          <label className="field">
            <span className="field-label">{t("Second value")}</span>
            <select className="select" value={B} onChange={(e) => setB(e.target.value)}>
              {values.map((r) => (
                <option key={r.ref} value={r.ref}>
                  {r.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {calc === "countRows" && (
          <label className="field">
            <span className="field-label">{t("Table")}</span>
            <select className="select" value={T} onChange={(e) => setTable(e.target.value)}>
              {tables.map((x) => (
                <option key={x} value={x}>
                  {x}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="field">
          <span className="field-label">{t("DAX")}</span>
          <pre className="quick-dax mono">{`${result[0]} = ${result[1]}`}</pre>
        </div>
      </div>
    </Dialog>
  );
}
