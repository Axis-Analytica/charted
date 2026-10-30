// The what-if parameter dialog (Power BI's "New parameter → Numeric range"): a calculated
// table of GENERATESERIES values whose column is named after the parameter, a measure
// with the value picked in it (SELECTEDVALUE, or the default), and optionally a slicer
// on the current page. Everything is one undoable commit.
import { useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { newId, newVisual, withField } from "../state/report";
import { uniqueTableName } from "./FormulaBar";
import { type ParameterSpec, parameterProblem, parameterTable } from "./parameters";

/** The changes that add the parameter's table and, given a page, a slicer of it. */
function parameterChanges(model: Model, p: ParameterSpec, page: Page | null): Change[] {
  const table = parameterTable(p, newId);
  const changes: Change[] = [{ op: "addTable", table, index: null }];
  if (page) {
    const withTable = { ...model, tables: [...model.tables, table] };
    const slicer = withField(withTable, newVisual(page, "slicer", null), { kind: "column", table: table.id, column: table.columns[0]!.id }, "field");
    if (slicer) changes.push({ op: "addVisual", page: page.id, visual: slicer, index: null });
  }
  return changes;
}

export function ParameterDialog({ model, page, onClose }: { model: Model; page: Page; onClose(): void }) {
  const { t } = useT();
  const [name, setName] = useState(() => uniqueTableName(model, t("Parameter")));
  const [decimal, setDecimal] = useState(false);
  const [min, setMin] = useState("0");
  const [max, setMax] = useState("20");
  const [step, setStep] = useState("1");
  const [def, setDef] = useState("0");
  const [slicer, setSlicer] = useState(true);
  const [busy, setBusy] = useState(false);

  const spec: ParameterSpec = { name: name.trim(), decimal, min: Number(min), max: Number(max), step: Number(step), default: Number(def) };
  const blank = [min, max, step, def].some((x) => x.trim() === "");
  const taken = model.tables.some((tb) => tb.name.toLowerCase() === spec.name.toLowerCase());
  const problem = blank ? null : !spec.name ? t("Enter a name.") : taken ? t("A table named %1 already exists.", spec.name) : parameterProblem(spec) && t(parameterProblem(spec)!);

  const save = async () => {
    if (blank || problem) return;
    setBusy(true);
    const changes = parameterChanges(model, spec, slicer ? page : null);
    const ok = await useApp.getState().commit("New parameter", changes);
    setBusy(false);
    if (ok) onClose();
  };

  const num = (label: string, value: string, set: (v: string) => void) => (
    <label className="field">
      <span className="field-label">{label}</span>
      <input className="input" type="number" step="any" aria-label={label} value={value} onChange={(e) => set(e.target.value)} />
    </label>
  );

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("What-if parameter")}
      width={420}
      footer={
        <>
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy || blank || !!problem} onPress={save}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="quick-measure parameter-dialog">
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          <input className="input" aria-label={t("Name")} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span className="field-label">{t("Data type")}</span>
          <select className="select" aria-label={t("Data type")} value={decimal ? "decimal" : "whole"} onChange={(e) => setDecimal(e.target.value === "decimal")}>
            <option value="whole">{t("Whole number")}</option>
            <option value="decimal">{t("Decimal number")}</option>
          </select>
        </label>
        {num(t("Minimum"), min, setMin)}
        {num(t("Maximum"), max, setMax)}
        {num(t("Increment"), step, setStep)}
        {num(t("Default"), def, setDef)}
        <label className="check">
          <input type="checkbox" checked={slicer} onChange={(e) => setSlicer(e.target.checked)} />
          {t("Add slicer to this page")}
        </label>
        <p className="field-hint">{t("Makes a table of the values, a measure %1 with the one picked, and a slicer to pick it.", `[${spec.name || name} Value]`)}</p>
        {problem && (
          <p className="field-error" role="alert">
            {problem}
          </p>
        )}
      </div>
    </Dialog>
  );
}
