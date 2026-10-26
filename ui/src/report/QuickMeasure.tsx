// The quick measure dialog: pick a calculation and its fields, see the DAX, and add it
// as a measure (then open in the formula bar to adjust). Opened from the Fields pane's
// menu, the formula bar and the command palette.
import { useMemo, useState } from "react";
import type { Measure } from "../bridge/generated/Measure";
import type { Model } from "../bridge/generated/Model";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { newId, useReport } from "../state/report";
import { uniqueName } from "./FormulaBar";
import { CALCS, type Calc, type FieldOption, fieldOptions, type Picks, quickMeasure } from "./quickMeasures";

export function QuickMeasureDialog({ model }: { model: Model }) {
  const home = useReport((s) => s.quick);
  return home === null ? null : <QuickMeasure key={home} model={model} home={home} />;
}

function QuickMeasure({ model, home }: { model: Model; home: string }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const set = useReport((s) => s.set);
  const close = () => set({ quick: null });
  const fields = useMemo(() => fieldOptions(model), [model]);
  const homeName = model.tables.find((x) => x.id === home)?.name ?? model.tables[0]?.name ?? "";
  // The home table's fields first.
  const own = (xs: FieldOption[]) => [...xs.filter((x) => x.table === homeName), ...xs.filter((x) => x.table !== homeName)];
  const values = own(fields.filter((r) => r.kind === "measure" || r.numeric));
  const columns = own(fields.filter((r) => r.kind === "column"));
  const numeric = columns.filter((r) => r.numeric);
  const dates = fields.filter((r) => r.date);
  const [calc, setCalc] = useState<Calc>("sum");
  const [picks, setPicks] = useState<Picks>(() => ({
    value: values[0]?.ref ?? "",
    second: values[1]?.ref ?? values[0]?.ref ?? "",
    column: columns[0]?.ref ?? "",
    table: homeName,
    date: dates[0]?.ref ?? "",
    category: columns.find((r) => !r.numeric && !r.date)?.ref ?? columns[0]?.ref ?? "",
    text: "",
    weight: "",
  }));
  const def = CALCS.find((c) => c.calc === calc)!;
  // A weighted average weighs a column by another of its table.
  const value = def.weighted && !numeric.some((r) => r.ref === picks.value) ? (numeric[0]?.ref ?? "") : picks.value;
  const weights = numeric.filter((r) => r.table === fields.find((x) => x.ref === value)?.table && r.ref !== value);
  const p: Picks = { ...picks, value, weight: weights.some((r) => r.ref === picks.weight) ? picks.weight : (weights[0]?.ref ?? "") };
  const result = quickMeasure(calc, p, fields);
  const missing = ((def.value || def.weighted) && !p.value) || (def.date && !p.date) || (def.category && !p.category) || (def.text && !p.text.trim()) || (def.weighted && !p.weight) || (def.column && !p.column);
  const pick = (k: keyof Picks) => (e: { target: { value: string } }) => setPicks((s) => ({ ...s, [k]: e.target.value }));
  const select = (k: keyof Picks, label: string, options: FieldOption[]) => (
    <label className="field">
      <span className="field-label">{label}</span>
      <select className="select" value={p[k]} onChange={pick(k)}>
        {options.map((r) => (
          <option key={r.ref} value={r.ref}>
            {r.label}
          </option>
        ))}
      </select>
    </label>
  );

  const add = async () => {
    const table = model.tables.find((x) => x.id === home) ?? model.tables[0];
    if (!table || missing) return;
    const id = newId();
    const measure: Measure = { id, name: uniqueName(model, result.name), expression: result.expression, language: "Dax", description: null, formatString: result.format, displayFolder: null, hidden: false };
    if (await commit("New quick measure", [{ op: "addMeasure", table: table.id, measure, index: null }])) set({ quick: null, formula: id, field: { kind: "measure", table: table.id, measure: id } });
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && close()}
      title={t("Quick measure")}
      width={520}
      footer={
        <>
          <Button variant="ghost" onPress={close}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={!!missing} onPress={add}>
            {t("Add")}
          </Button>
        </>
      }
    >
      <div className="quick-measure">
        <label className="field">
          <span className="field-label">{t("Calculation")}</span>
          <select className="select" value={calc} onChange={(e) => setCalc(e.target.value as Calc)} autoFocus>
            {[...new Set(CALCS.map((c) => c.group))].map((g) => (
              <optgroup key={g} label={t(g)}>
                {CALCS.filter((c) => c.group === g).map((c) => (
                  <option key={c.calc} value={c.calc}>
                    {t(c.label)}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>
        {def.value && select("value", def.second ? t("First value") : t("Value"), values)}
        {def.weighted && select("value", t("Value"), numeric)}
        {def.weighted && select("weight", t("Weight"), weights)}
        {def.second && select("second", t("Second value"), values)}
        {def.column && select("column", t("Column"), columns)}
        {def.category && select("category", t("Category"), columns)}
        {def.text && (
          <label className="field">
            <span className="field-label">{t("Equal to")}</span>
            <input className="input" value={p.text} onChange={pick("text")} placeholder={t("A value of the category")} />
          </label>
        )}
        {def.date && (dates.length ? select("date", t("Date"), dates) : <p className="pane-hint">{t("This needs a date column. Model view → New date table makes one.")}</p>)}
        {def.table && (
          <label className="field">
            <span className="field-label">{t("Table")}</span>
            <select className="select" value={p.table} onChange={pick("table")}>
              {model.tables.map((x) => (
                <option key={x.id} value={x.name}>
                  {x.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="field">
          <span className="field-label">{t("DAX")}</span>
          <pre className="quick-dax mono">{`${result.name} = ${result.expression}`}</pre>
        </div>
      </div>
    </Dialog>
  );
}
