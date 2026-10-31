// A font list for the Format pane and the theme editor: the theme's font, the user's own
// fonts, the bundled ones and Power BI's names, with "Add a font…" at the end.
import { errorMessage } from "../bridge";
import { addFont, BUNDLED, POWER_BI, useFonts } from "../design/fonts";
import { useT } from "../i18n";
import { toast } from "../state/toasts";

const ADD = "\u0000add";

export function FontPicker({ value, onChange, label, mixed }: { value: string; onChange: (font: string) => void; label: string; mixed?: boolean }) {
  const { t } = useT();
  const yours = useFonts((s) => s.families);
  const known = new Set([...yours, ...Object.keys(BUNDLED), ...Object.keys(POWER_BI)]);
  return (
    <select
      className="select"
      aria-label={label}
      value={mixed ? "\u0000mixed" : value}
      onChange={async (e) => {
        const v = e.target.value;
        if (v !== ADD) return onChange(v);
        e.target.value = value;
        try {
          const family = await addFont(t("Add a font"));
          if (family) onChange(family);
        } catch (err) {
          toast({ kind: "error", title: errorMessage(err) });
        }
      }}
    >
      {mixed && <option value={"\u0000mixed"}>{t("Mixed")}</option>}
      <option value="">{t("Theme font")}</option>
      {value && !known.has(value) && <option value={value}>{value}</option>}
      {yours.length > 0 && (
        <optgroup label={t("Your fonts")}>
          {yours.map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </optgroup>
      )}
      <optgroup label={t("Included with charted")}>
        {Object.keys(BUNDLED).map((f) => (
          <option key={f} value={f}>
            {f}
          </option>
        ))}
      </optgroup>
      <optgroup label={t("Power BI fonts (a similar font if not installed)")}>
        {Object.keys(POWER_BI).map((f) => (
          <option key={f} value={f}>
            {f}
          </option>
        ))}
      </optgroup>
      <option value={ADD}>{t("Add a font…")}</option>
    </select>
  );
}
