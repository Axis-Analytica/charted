// The formula bar: edit a measure as `Name = expression`, checked against the model as
// you type (parse, names, supported functions, cycles). Enter commits, Shift+Enter
// adds a line, Escape cancels. Quick measures write the DAX for common calculations.
import { Check, Sparkles, Trash2, WandSparkles, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DaxCheck } from "../bridge/generated/DaxCheck";
import type { Measure } from "../bridge/generated/Measure";
import type { Model } from "../bridge/generated/Model";
import { useT } from "../i18n";
import { IconButton } from "../kit/Button";
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

export function uniqueName(model: Model, base: string): string {
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
        <IconButton label={t("Quick measure")} icon={<Sparkles size={15} />} onPress={() => set({ quick: home.id })} />
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
    </div>
  );
}
