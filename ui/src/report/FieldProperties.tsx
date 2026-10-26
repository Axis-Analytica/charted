// The Fields pane's properties dialog for a measure, a column or a hierarchy: name,
// description, display folder and hidden for all; format and home table for measures;
// format, sort by, summarization and data category for columns; levels for hierarchies.
// Everything applies together on OK as one undo step.
import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Column } from "../bridge/generated/Column";
import type { ColumnProperty } from "../bridge/generated/ColumnProperty";
import type { Hierarchy } from "../bridge/generated/Hierarchy";
import type { Measure } from "../bridge/generated/Measure";
import type { Model } from "../bridge/generated/Model";
import type { Summarize } from "../bridge/generated/Summarize";
import type { Table } from "../bridge/generated/Table";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { isNumeric } from "../views/data/DataGrid";
import { CATEGORIES, FormatPicker, SUMMARIZE } from "../views/data/FormatPicker";
import { folderPath, folderString, moveMeasure, tableFolders } from "./fieldTree";

export type PropsTarget = { kind: "measure" | "column" | "hierarchy"; table: string; id: string };

export function FieldProperties({ model, target, onClose }: { model: Model; target: PropsTarget; onClose(): void }) {
  const table = model.tables.find((x) => x.id === target.table);
  if (!table) return null;
  const key = `${target.kind}:${target.id}`;
  if (target.kind === "measure") {
    const m = table.measures.find((x) => x.id === target.id);
    return m ? <MeasureProps key={key} model={model} table={table} measure={m} onClose={onClose} /> : null;
  }
  if (target.kind === "column") {
    const c = table.columns.find((x) => x.id === target.id);
    return c ? <ColumnProps key={key} table={table} column={c} onClose={onClose} /> : null;
  }
  const h = table.hierarchies?.find((x) => x.id === target.id);
  return h ? <HierarchyProps key={key} table={table} hierarchy={h} onClose={onClose} /> : null;
}

/** Name, description, display folder and hidden: what every kind shares. */
interface Common {
  name: string;
  description: string;
  folder: string;
  hidden: boolean;
}

const common = (x: { name: string; description?: string | null; displayFolder?: string | null; hidden: boolean }): Common => ({
  name: x.name,
  description: x.description ?? "",
  folder: x.displayFolder ?? "",
  hidden: x.hidden,
});
const description = (c: Common) => c.description.trim() || null;
const folder = (c: Common) => folderString(folderPath(c.folder));

function CommonFields({ table, value, onChange, children }: { table: Table; value: Common; onChange(c: Common): void; children?: React.ReactNode }) {
  const { t } = useT();
  const listId = `folders-${table.id}`;
  return (
    <>
      <label className="field">
        <span className="field-label">{t("Name")}</span>
        <input className="input" aria-label={t("Name")} value={value.name} autoFocus onChange={(e) => onChange({ ...value, name: e.target.value })} />
      </label>
      <label className="field">
        <span className="field-label">{t("Description")}</span>
        <textarea className="input" rows={2} aria-label={t("Description")} value={value.description} placeholder={t("Shown when hovering the field in reports")} onChange={(e) => onChange({ ...value, description: e.target.value })} />
      </label>
      {children}
      <label className="field">
        <span className="field-label">{t("Display folder")}</span>
        <input className="input" aria-label={t("Display folder")} list={listId} value={value.folder} placeholder={t("Folder\\Subfolder")} onChange={(e) => onChange({ ...value, folder: e.target.value })} />
        <datalist id={listId}>
          {tableFolders(table).map((f) => (
            <option key={f} value={f} />
          ))}
        </datalist>
      </label>
      <label className="check">
        <input type="checkbox" checked={value.hidden} onChange={(e) => onChange({ ...value, hidden: e.target.checked })} />
        {t("Hidden in reports")}
      </label>
    </>
  );
}

function PropsDialog({ title, onClose, onSave, children, invalid }: { title: string; onClose(): void; onSave(): Promise<boolean>; children: React.ReactNode; invalid?: boolean }) {
  const { t } = useT();
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    const ok = await onSave();
    setBusy(false);
    if (ok) onClose();
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={title}
      width={460}
      footer={
        <>
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy || invalid} onPress={save}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <form
        className="quick-measure field-props"
        onSubmit={(e) => {
          e.preventDefault();
          if (!invalid) save();
        }}
      >
        {children}
        <button type="submit" hidden />
      </form>
    </Dialog>
  );
}

function MeasureProps({ model, table, measure, onClose }: { model: Model; table: Table; measure: Measure; onClose(): void }) {
  const { t } = useT();
  const [c, setC] = useState(() => common(measure));
  const [format, setFormat] = useState(measure.formatString ?? null);
  const [home, setHome] = useState(table.id);
  const save = () =>
    apply("Change measure properties", (latest) => {
      const m = latest.tables.find((x) => x.id === table.id)?.measures.find((x) => x.id === measure.id);
      if (!m) return null;
      const next: Measure = { ...m, name: c.name.trim(), description: description(c), formatString: format, displayFolder: folder(c), hidden: c.hidden };
      if (home !== table.id) return moveMeasure(latest, m.id, table.id, home, () => next);
      return JSON.stringify(next) === JSON.stringify(m) ? [] : [{ op: "updateMeasure", table: table.id, measure: next }];
    });
  return (
    <PropsDialog title={t("Measure properties")} onClose={onClose} onSave={save} invalid={!c.name.trim()}>
      <CommonFields table={table} value={c} onChange={setC}>
        <FormatPicker dataType="Double" value={format} onChange={setFormat} />
        <label className="field">
          <span className="field-label">{t("Home table")}</span>
          <select className="select" aria-label={t("Home table")} value={home} onChange={(e) => setHome(e.target.value)}>
            {model.tables.map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
          </select>
        </label>
      </CommonFields>
    </PropsDialog>
  );
}

/** Commits what `build` returns; true when it landed or there was nothing to change. */
async function apply(label: string, build: (model: Model) => Change[] | null): Promise<boolean> {
  let empty = false;
  const ok = await useApp.getState().edit(label, (m) => {
    const changes = build(m);
    empty = !!changes && !changes.length;
    return changes;
  });
  return ok || empty;
}

function ColumnProps({ table, column, onClose }: { table: Table; column: Column; onClose(): void }) {
  const { t } = useT();
  const [c, setC] = useState(() => common(column));
  const [format, setFormat] = useState(column.formatString ?? null);
  const [sortBy, setSortBy] = useState(column.sortBy ?? null);
  const [summarize, setSummarize] = useState<Summarize>(column.summarizeBy);
  const [category, setCategory] = useState(column.dataCategory ?? null);
  const numeric = isNumeric(column.dataType);
  const save = () =>
    apply("Change column properties", (latest) => {
      const now = latest.tables.find((x) => x.id === table.id)?.columns.find((x) => x.id === column.id);
      if (!now) return null;
      const changes: Change[] = [];
      const set = (property: ColumnProperty) => changes.push({ op: "setColumn", table: table.id, column: column.id, property });
      if (c.name.trim() !== now.name) changes.push({ op: "renameColumn", table: table.id, column: column.id, name: c.name.trim() });
      if (description(c) !== (now.description ?? null)) set({ name: "description", value: description(c) });
      if (format !== (now.formatString ?? null)) set({ name: "formatString", value: format });
      if (folder(c) !== (now.displayFolder ?? null)) set({ name: "displayFolder", value: folder(c) });
      if (c.hidden !== now.hidden) set({ name: "hidden", value: c.hidden });
      if (sortBy !== (now.sortBy ?? null)) set({ name: "sortBy", value: sortBy });
      if (summarize !== now.summarizeBy) set({ name: "summarizeBy", value: summarize });
      if (category !== (now.dataCategory ?? null)) set({ name: "dataCategory", value: category });
      return changes;
    });
  return (
    <PropsDialog title={t("Column properties")} onClose={onClose} onSave={save} invalid={!c.name.trim()}>
      <CommonFields table={table} value={c} onChange={setC}>
        <FormatPicker dataType={column.dataType} value={format} onChange={setFormat} />
        <label className="field">
          <span className="field-label">{t("Sort by column")}</span>
          <select className="select" aria-label={t("Sort by column")} value={sortBy ?? ""} onChange={(e) => setSortBy(e.target.value || null)}>
            <option value="">{t("(This column)")}</option>
            {table.columns
              .filter((x) => x.id !== column.id)
              .map((x) => (
                <option key={x.id} value={x.id}>
                  {x.name}
                </option>
              ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Summarize by")}</span>
          <select className="select" aria-label={t("Summarize by")} value={summarize} onChange={(e) => setSummarize(e.target.value as Summarize)}>
            {SUMMARIZE.filter((s) => numeric || !s.numericOnly).map((s) => (
              <option key={s.value} value={s.value}>
                {t(s.label)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Data category")}</span>
          <select className="select" aria-label={t("Data category")} value={category ?? ""} onChange={(e) => setCategory(e.target.value || null)}>
            <option value="">{t("Uncategorized")}</option>
            {CATEGORIES.map((x) => (
              <option key={x.value} value={x.value}>
                {t(x.label)}
              </option>
            ))}
            {category && !CATEGORIES.some((x) => x.value === category) && <option value={category}>{category}</option>}
          </select>
        </label>
      </CommonFields>
    </PropsDialog>
  );
}

function HierarchyProps({ table, hierarchy, onClose }: { table: Table; hierarchy: Hierarchy; onClose(): void }) {
  const { t } = useT();
  const [c, setC] = useState(() => common(hierarchy));
  const [levels, setLevels] = useState(hierarchy.levels);
  const columnName = (id: string) => table.columns.find((x) => x.id === id)?.name ?? "?";
  const free = table.columns.filter((x) => !levels.some((l) => l.column === x.id));
  const move = (i: number, by: number) =>
    setLevels((ls) => {
      const next = [...ls];
      const [l] = next.splice(i, 1);
      next.splice(i + by, 0, l!);
      return next;
    });
  const names = levels.map((l) => l.name.trim().toLowerCase());
  const invalid = !c.name.trim() || !levels.length || names.some((n, i) => !n || names.indexOf(n) !== i);
  const save = () =>
    apply("Change hierarchy", (latest) => {
      const now = latest.tables.find((x) => x.id === table.id)?.hierarchies?.find((x) => x.id === hierarchy.id);
      if (!now) return null;
      const next: Hierarchy = { ...now, name: c.name.trim(), description: description(c), displayFolder: folder(c), hidden: c.hidden, levels: levels.map((l) => ({ ...l, name: l.name.trim() })) };
      return JSON.stringify(next) === JSON.stringify(now) ? [] : [{ op: "updateHierarchy", table: table.id, hierarchy: next }];
    });
  return (
    <PropsDialog title={t("Hierarchy properties")} onClose={onClose} onSave={save} invalid={invalid}>
      <CommonFields table={table} value={c} onChange={setC}>
        <div className="field">
          <span className="field-label">{t("Levels")}</span>
          <ol className="hierarchy-levels" aria-label={t("Levels")}>
            {levels.map((l, i) => (
              <li key={l.column}>
                <input
                  className="input"
                  aria-label={t("Level %1 name", i + 1)}
                  value={l.name}
                  onChange={(e) => setLevels((ls) => ls.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))}
                />
                <span className="hierarchy-level-column" title={columnName(l.column)}>
                  {columnName(l.column)}
                </span>
                <IconButton label={t("Move up")} icon={<ArrowUp size={13} />} isDisabled={i === 0} onPress={() => move(i, -1)} />
                <IconButton label={t("Move down")} icon={<ArrowDown size={13} />} isDisabled={i === levels.length - 1} onPress={() => move(i, 1)} />
                <IconButton label={t("Remove level %1", l.name)} icon={<X size={13} />} onPress={() => setLevels((ls) => ls.filter((_, j) => j !== i))} />
              </li>
            ))}
          </ol>
          {free.length > 0 && (
            <select
              className="select"
              aria-label={t("Add level")}
              value=""
              onChange={(e) => {
                const col = table.columns.find((x) => x.id === e.target.value);
                if (col) setLevels((ls) => [...ls, { name: ls.some((l) => l.name.toLowerCase() === col.name.toLowerCase()) ? `${col.name} 2` : col.name, column: col.id }]);
              }}
            >
              <option value="">{t("Add level…")}</option>
              {free.map((x) => (
                <option key={x.id} value={x.id}>
                  {x.name}
                </option>
              ))}
            </select>
          )}
        </div>
      </CommonFields>
    </PropsDialog>
  );
}
