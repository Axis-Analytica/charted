import { X } from "lucide-react";
import { useState } from "react";
import type { Column } from "../../bridge/generated/Column";
import type { ColumnProperty } from "../../bridge/generated/ColumnProperty";
import type { DataType } from "../../bridge/generated/DataType";
import type { Summarize } from "../../bridge/generated/Summarize";
import type { Table } from "../../bridge/generated/Table";
import { formatValue, isDateFormat } from "../../format";
import { useLanguage, useT } from "../../i18n";
import { IconButton } from "../../kit/Button";
import { useApp } from "../../state/app";
import { DataTypeIcon } from "../icons";
import { isNumeric } from "./DataGrid";

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

const NUMBER_FORMATS = [
  { format: "#,0", label: "Whole number" },
  { format: "#,0.00", label: "Decimal number" },
  { format: "\\$#,0.00;(\\$#,0.00)", label: "Currency" },
  { format: "0.0%", label: "Percentage" },
  { format: "#,0.0,K", label: "Thousands" },
  { format: "#,0.0,,M", label: "Millions" },
  { format: "0.00E+00", label: "Scientific" },
];
// Date patterns are shown as written; only named formats carry a translatable label.
const DATE_FORMATS: { format: string; label?: string }[] = [
  { format: "yyyy-mm-dd" },
  { format: "dd/mm/yyyy" },
  { format: "m/d/yyyy" },
  { format: "d mmm yyyy" },
  { format: "mmmm yyyy" },
  { format: "Long Date", label: "Long date" },
];
const DATETIME_FORMATS: { format: string; label?: string }[] = [{ format: "yyyy-mm-dd hh:mm" }, { format: "General Date", label: "General date" }, ...DATE_FORMATS];

const SUMMARIZE: { value: Summarize; label: string; numericOnly?: boolean }[] = [
  { value: "Default", label: "Default" },
  { value: "None", label: "Don't summarize" },
  { value: "Sum", label: "Sum", numericOnly: true },
  { value: "Average", label: "Average", numericOnly: true },
  { value: "Min", label: "Minimum" },
  { value: "Max", label: "Maximum" },
  { value: "Count", label: "Count" },
  { value: "DistinctCount", label: "Count (distinct)" },
];

const CATEGORIES: { value: string; label: string }[] = [
  { value: "Address", label: "Address" },
  { value: "Place", label: "Place" },
  { value: "City", label: "City" },
  { value: "County", label: "County" },
  { value: "StateOrProvince", label: "State or province" },
  { value: "PostalCode", label: "Postal code" },
  { value: "Country", label: "Country/region" },
  { value: "Continent", label: "Continent" },
  { value: "Latitude", label: "Latitude" },
  { value: "Longitude", label: "Longitude" },
  { value: "WebUrl", label: "Web URL" },
  { value: "ImageUrl", label: "Image URL" },
  { value: "Barcode", label: "Barcode" },
];

const SAMPLE_DATE = Date.UTC(2025, 2, 7, 14, 5, 9);

/** A text input that commits on Enter or blur, and reverts on Escape. */
function CommitInput({ value, onCommit, label, multiline, mono, placeholder, onDraft }: { value: string; onCommit(v: string): void; label: string; multiline?: boolean; mono?: boolean; placeholder?: string; onDraft?(v: string): void }) {
  const [draft, setDraft] = useState(value);
  const commit = () => draft !== value && onCommit(draft);
  const props = {
    className: `input ${mono ? "mono" : ""}`,
    "aria-label": label,
    value: draft,
    placeholder,
    onChange: (e: { target: { value: string } }) => (setDraft(e.target.value), onDraft?.(e.target.value)),
    onBlur: commit,
    onKeyDown: (e: React.KeyboardEvent) => {
      e.stopPropagation();
      if (e.key === "Enter" && !(multiline && e.shiftKey)) (e.preventDefault(), commit());
      if (e.key === "Escape") (setDraft(value), onDraft?.(value));
    },
  };
  return multiline ? <textarea rows={2} {...props} /> : <input {...props} />;
}

export function ColumnProperties({ table, column, onClose }: { table: Table; column: Column; onClose(): void }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const commit = useApp((s) => s.commit);
  const [draftFormat, setDraftFormat] = useState<string | null>(null);
  const set = (label: string, property: ColumnProperty) => commit(label, [{ op: "setColumn", table: table.id, column: column.id, property }]);

  const numeric = isNumeric(column.dataType);
  const temporal = column.dataType === "Date" || column.dataType === "DateTime" || column.dataType === "DateTimeTz";
  const presets = numeric ? NUMBER_FORMATS : column.dataType === "Date" ? DATE_FORMATS : temporal ? DATETIME_FORMATS : [];
  const format = draftFormat ?? column.formatString ?? "";
  const preset = presets.find((p) => p.format === (column.formatString ?? ""));
  const preview = (() => {
    if (numeric) {
      const v = column.dataType === "Int64" ? 1234567 : 1234.5678;
      return `${formatValue(v, column.dataType, format, locale)}   ${formatValue(-v, column.dataType, format, locale)}   ${formatValue(0.256, column.dataType, format, locale)}`;
    }
    if (temporal || column.dataType === "Time") return formatValue(SAMPLE_DATE, column.dataType, format && isDateFormat(format) ? format : null, locale);
    return null;
  })();
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
        {presets.length > 0 && (
          <>
            <label className="field">
              <span className="field-label">{t("Format")}</span>
              <select
                className="select"
                aria-label={t("Format")}
                value={column.formatString ? (preset ? preset.format : "custom") : ""}
                onChange={(e) => {
                  if (e.target.value === "custom") return;
                  set("Change format", { name: "formatString", value: e.target.value || null });
                }}
              >
                <option value="">{t("General")}</option>
                {presets.map((p) => (
                  <option key={p.format} value={p.format}>
                    {p.label ? t(p.label) : p.format}
                  </option>
                ))}
                <option value="custom">{t("Custom")}</option>
              </select>
            </label>
            <label className="field">
              <span className="field-label">{t("Format string")}</span>
              <CommitInput
                key={column.formatString ?? ""}
                mono
                label={t("Format string")}
                value={column.formatString ?? ""}
                placeholder={numeric ? "#,0.00" : "yyyy-mm-dd"}
                onDraft={setDraftFormat}
                onCommit={(f) => {
                  setDraftFormat(null);
                  set("Change format", { name: "formatString", value: f.trim() || null });
                }}
              />
            </label>
            {preview && (
              <div className="props-preview" aria-live="polite">
                <span className="field-label">{t("Preview")}</span>
                <span className="mono tabular">{preview}</span>
              </div>
            )}
          </>
        )}

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
