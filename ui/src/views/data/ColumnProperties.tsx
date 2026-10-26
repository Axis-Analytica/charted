import { X } from "lucide-react";
import type { Column } from "../../bridge/generated/Column";
import type { ColumnProperty } from "../../bridge/generated/ColumnProperty";
import type { DataType } from "../../bridge/generated/DataType";
import type { Summarize } from "../../bridge/generated/Summarize";
import type { Table } from "../../bridge/generated/Table";
import { useT } from "../../i18n";
import { IconButton } from "../../kit/Button";
import { useApp } from "../../state/app";
import { DataTypeIcon } from "../icons";
import { isNumeric } from "./DataGrid";
import { CATEGORIES, CommitInput, FormatPicker, SUMMARIZE } from "./FormatPicker";

const TYPES: { type: DataType; label: string }[] = [
  { type: "String", label: "Text" },
  { type: "Int64", label: "Whole number" },
  { type: "Double", label: "Decimal number" },
  { type: "Decimal", label: "Fixed decimal number" },
  { type: "Boolean", label: "True/false" },
  { type: "Date", label: "Date" },
  { type: "DateTime", label: "Date/time" },
  { type: "DateTimeTz", label: "Date/time/timezone" },
  { type: "Time", label: "Time" },
];

export function ColumnProperties({ table, column, onClose }: { table: Table; column: Column; onClose(): void }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const set = (label: string, property: ColumnProperty) => commit(label, [{ op: "setColumn", table: table.id, column: column.id, property }]);

  const numeric = isNumeric(column.dataType);
  const sortTargets = table.columns.filter((c) => c.id !== column.id && c.kind.kind === "data");
  const source = column.kind.kind === "data" ? column.kind.sourceColumn : null;

  return (
    <aside className="props" aria-label={t("Column properties")}>
      <div className="props-head">
        <DataTypeIcon type={column.dataType} />
        <span className="props-title">{column.name}</span>
        <IconButton label={t("Hide properties")} icon={<X size={14} />} onPress={onClose} />
      </div>
      <div className="props-body" key={column.id}>
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          <CommitInput key={column.name} label={t("Name")} value={column.name} onCommit={(name) => name.trim() && commit("Rename column", [{ op: "renameColumn", table: table.id, column: column.id, name: name.trim() }])} />
        </label>
        <label className="field">
          <span className="field-label">{t("Description")}</span>
          <CommitInput key={column.description ?? ""} label={t("Description")} multiline value={column.description ?? ""} placeholder={t("Shown when hovering the field in reports")} onCommit={(d) => set("Change description", { name: "description", value: d.trim() || null })} />
        </label>

        <h3 className="props-section">{t("Formatting")}</h3>
        <label className="field">
          <span className="field-label">{t("Data type")}</span>
          <select className="select" value={column.dataType} aria-label={t("Data type")} onChange={(e) => set("Change data type", { name: "dataType", value: e.target.value as DataType })}>
            {TYPES.map((x) => (
              <option key={x.type} value={x.type}>
                {t(x.label)}
              </option>
            ))}
          </select>
        </label>
        <FormatPicker dataType={column.dataType} value={column.formatString ?? null} onChange={(f) => set("Change format", { name: "formatString", value: f })} />

        <h3 className="props-section">{t("Properties")}</h3>
        <label className="field">
          <span className="field-label">{t("Summarize by")}</span>
          <select className="select" aria-label={t("Summarize by")} value={column.summarizeBy} onChange={(e) => set("Change summarization", { name: "summarizeBy", value: e.target.value as Summarize })}>
            {SUMMARIZE.filter((s) => numeric || !s.numericOnly).map((s) => (
              <option key={s.value} value={s.value}>
                {t(s.label)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Data category")}</span>
          <select className="select" aria-label={t("Data category")} value={column.dataCategory ?? ""} onChange={(e) => set("Change data category", { name: "dataCategory", value: e.target.value || null })}>
            <option value="">{t("Uncategorized")}</option>
            {CATEGORIES.map((c) => (
              <option key={c.value} value={c.value}>
                {t(c.label)}
              </option>
            ))}
            {column.dataCategory && !CATEGORIES.some((c) => c.value === column.dataCategory) && <option value={column.dataCategory}>{column.dataCategory}</option>}
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Sort by column")}</span>
          <select className="select" aria-label={t("Sort by column")} value={column.sortBy ?? ""} onChange={(e) => set("Change sort by column", { name: "sortBy", value: e.target.value || null })}>
            <option value="">{t("(This column)")}</option>
            {sortTargets.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        <label className="check">
          <input type="checkbox" checked={column.hidden} onChange={(e) => set(e.target.checked ? "Hide column" : "Show column", { name: "hidden", value: e.target.checked })} />
          {t("Hidden in reports")}
        </label>
        {source && (
          <p className="props-foot">
            {t("Loaded from column %1", source)}
          </p>
        )}
      </div>
    </aside>
  );
}
