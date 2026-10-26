// The format picker (presets, a format string and a preview) and the option lists the
// Data view's column properties and the Fields pane's properties dialog share.
import { useState } from "react";
import type { DataType } from "../../bridge/generated/DataType";
import type { Summarize } from "../../bridge/generated/Summarize";
import { formatValue, isDateFormat } from "../../format";
import { useLanguage, useT } from "../../i18n";
import { isNumeric } from "./DataGrid";

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

export const SUMMARIZE: { value: Summarize; label: string; numericOnly?: boolean }[] = [
  { value: "Default", label: "Default" },
  { value: "None", label: "Don't summarize" },
  { value: "Sum", label: "Sum", numericOnly: true },
  { value: "Average", label: "Average", numericOnly: true },
  { value: "Min", label: "Minimum" },
  { value: "Max", label: "Maximum" },
  { value: "Count", label: "Count" },
  { value: "DistinctCount", label: "Count (distinct)" },
];

export const CATEGORIES: { value: string; label: string }[] = [
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
export function CommitInput({ value, onCommit, label, multiline, mono, placeholder, onDraft }: { value: string; onCommit(v: string): void; label: string; multiline?: boolean; mono?: boolean; placeholder?: string; onDraft?(v: string): void }) {
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

/** Format presets for a data type, a format string and a preview of sample values. */
export function FormatPicker({ dataType, value, onChange }: { dataType: DataType; value: string | null; onChange(v: string | null): void }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const [draftFormat, setDraftFormat] = useState<string | null>(null);
  const numeric = isNumeric(dataType);
  const temporal = dataType === "Date" || dataType === "DateTime" || dataType === "DateTimeTz";
  const presets = numeric ? NUMBER_FORMATS : dataType === "Date" ? DATE_FORMATS : temporal ? DATETIME_FORMATS : [];
  const format = draftFormat ?? value ?? "";
  const preset = presets.find((p) => p.format === (value ?? ""));
  const preview = (() => {
    if (numeric) {
      const v = dataType === "Int64" ? 1234567 : 1234.5678;
      return `${formatValue(v, dataType, format, locale)}   ${formatValue(-v, dataType, format, locale)}   ${formatValue(0.256, dataType, format, locale)}`;
    }
    if (temporal || dataType === "Time") return formatValue(SAMPLE_DATE, dataType, format && isDateFormat(format) ? format : null, locale);
    return null;
  })();
  if (!presets.length) return null;
  return (
    <>
      <label className="field">
        <span className="field-label">{t("Format")}</span>
        <select
          className="select"
          aria-label={t("Format")}
          value={value ? (preset ? preset.format : "custom") : ""}
          onChange={(e) => {
            if (e.target.value === "custom") return;
            onChange(e.target.value || null);
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
          key={value ?? ""}
          mono
          label={t("Format string")}
          value={value ?? ""}
          placeholder={numeric ? "#,0.00" : "yyyy-mm-dd"}
          onDraft={setDraftFormat}
          onCommit={(f) => {
            setDraftFormat(null);
            onChange(f.trim() || null);
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
  );
}
