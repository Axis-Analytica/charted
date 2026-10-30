// The parameter dialog (Power BI's "New parameter"), in two kinds:
// - Numeric range (what-if): a calculated table of GENERATESERIES values whose column is
//   named after the parameter, and a measure with the value picked in it (SELECTEDVALUE,
//   or the default).
// - Fields: a field parameter, a table of the chosen measures and columns; a visual using
//   it shows the fields picked in its slicer (the core expands it, see fieldparams.rs).
// Either can add a slicer to the current page. Everything is one undoable commit.
import { useMemo, useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Table } from "../bridge/generated/Table";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { newId, newVisual, withField } from "../state/report";
import { uniqueTableName } from "./FormulaBar";
import { fieldParameterTable, type ParameterField, type ParameterSpec, parameterProblem, parameterTable } from "./parameters";

/** The changes that add a parameter's table and, given a page, a slicer of its first column. */
function parameterChanges(model: Model, table: Table, page: Page | null): Change[] {
  const changes: Change[] = [{ op: "addTable", table, index: null }];
  if (page) {
    const withTable = { ...model, tables: [...model.tables, table] };
    const slicer = withField(withTable, newVisual(page, "slicer", null), { kind: "column", table: table.id, column: table.columns[0]!.id }, "field");
    if (slicer) changes.push({ op: "addVisual", page: page.id, visual: slicer, index: null });
  }
  return changes;
}

/** The measures and visible columns a field parameter can offer, by table. */
function offered(model: Model): { table: string; fields: ParameterField[] }[] {
  return model.tables
    .filter((tb) => !tb.hidden)
    .map((tb) => ({
      table: tb.name,
      fields: [...tb.measures.filter((m) => !m.hidden).map((m) => m.name), ...tb.columns.filter((c) => !c.hidden).map((c) => c.name)].map((name) => ({ label: name, table: tb.name, name })),
    }))
    .filter((g) => g.fields.length > 0);
}

const key = (f: ParameterField) => `${f.table}\n${f.name}`;

export function ParameterDialog({ model, page, onClose }: { model: Model; page: Page; onClose(): void }) {
  const { t } = useT();
  const [kind, setKind] = useState<"range" | "fields">("range");
  const [name, setName] = useState(() => uniqueTableName(model, t("Parameter")));
  const [decimal, setDecimal] = useState(false);
  const [min, setMin] = useState("0");
  const [max, setMax] = useState("20");
  const [step, setStep] = useState("1");
  const [def, setDef] = useState("0");
  const [fields, setFields] = useState<ParameterField[]>([]);
  const [search, setSearch] = useState("");
  const [slicer, setSlicer] = useState(true);
  const [busy, setBusy] = useState(false);
  const groups = useMemo(() => offered(model), [model]);

  const spec: ParameterSpec = { name: name.trim(), decimal, min: Number(min), max: Number(max), step: Number(step), default: Number(def) };
  const blank = kind === "range" ? [min, max, step, def].some((x) => x.trim() === "") : fields.length === 0;
  const taken = model.tables.some((tb) => tb.name.toLowerCase() === spec.name.toLowerCase());
  const rangeProblem = kind === "range" ? parameterProblem(spec) : null;
  const problem = blank ? null : !spec.name ? t("Enter a name.") : taken ? t("A table named %1 already exists.", spec.name) : rangeProblem && t(rangeProblem);

  const save = async () => {
    if (blank || problem) return;
    setBusy(true);
    const table = kind === "range" ? parameterTable(spec, newId) : fieldParameterTable(spec.name, fields, newId);
    const ok = await useApp.getState().commit("New parameter", parameterChanges(model, table, slicer ? page : null));
    setBusy(false);
    if (ok) onClose();
  };

  const num = (label: string, value: string, set: (v: string) => void) => (
    <label className="field">
      <span className="field-label">{label}</span>
      <input className="input" type="number" step="any" aria-label={label} value={value} onChange={(e) => set(e.target.value)} />
    </label>
  );
  const q = search.trim().toLowerCase();
  const toggle = (f: ParameterField, on: boolean) => setFields((fs) => (on ? [...fs, f] : fs.filter((x) => key(x) !== key(f))));

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("New parameter")}
      width={kind === "range" ? 420 : 480}
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
          <span className="field-label">{t("Kind")}</span>
          <select className="select" aria-label={t("Kind")} value={kind} onChange={(e) => setKind(e.target.value as "range" | "fields")}>
            <option value="range">{t("Numeric range")}</option>
            <option value="fields">{t("Fields")}</option>
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          <input className="input" aria-label={t("Name")} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        </label>
        {kind === "range" ? (
          <>
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
          </>
        ) : (
          <div className="field">
            <span className="field-label">{t("Fields, in the order picked")}</span>
            <input className="input" aria-label={t("Search fields")} placeholder={t("Search")} value={search} onChange={(e) => setSearch(e.target.value)} />
            <ul className="group-values" aria-label={t("Fields to offer")}>
              {groups.map((g) =>
                g.fields
                  .filter((f) => !q || f.name.toLowerCase().includes(q) || g.table.toLowerCase().includes(q))
                  .map((f) => {
                    const at = fields.findIndex((x) => key(x) === key(f));
                    return (
                      <li key={key(f)}>
                        <label className="check">
                          <input type="checkbox" checked={at >= 0} onChange={(e) => toggle(f, e.target.checked)} />
                          {f.name}
                          <span className="pane-hint">
                            {" "}
                            {g.table}
                            {at >= 0 ? ` · ${at + 1}` : ""}
                          </span>
                        </label>
                      </li>
                    );
                  }),
              )}
            </ul>
          </div>
        )}
        <label className="check">
          <input type="checkbox" checked={slicer} onChange={(e) => setSlicer(e.target.checked)} />
          {t("Add slicer to this page")}
        </label>
        <p className="field-hint">
          {kind === "range"
            ? t("Makes a table of the values, a measure %1 with the one picked, and a slicer to pick it.", `[${spec.name || name} Value]`)
            : t("Put %1 in a visual: it shows the fields picked in the slicer, or all of them.", spec.name || name)}
        </p>
        {problem && (
          <p className="field-error" role="alert">
            {problem}
          </p>
        )}
      </div>
    </Dialog>
  );
}
