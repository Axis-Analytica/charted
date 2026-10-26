// The multi-row card (Power BI's new card): several values, each a callout with its
// label, laid out in a row that wraps (or in a set number of columns).
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { projectionName } from "../../state/report";
import type { VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { fmt, projectionFormat, withUnits } from "./common";

export function MultiRowCard({ visual, model, data }: { visual: Visual; model: Model; data: VisualData }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const def = visualDef("multiRowCard");
  const o = visual.objects;
  const fields = visual.roles.values ?? [];
  const units = prop<string>(o, def, "callout", "displayUnits");
  const size = prop<number>(o, def, "callout", "fontSize");
  const columns = prop<number>(o, def, "layout", "columns");
  const accent = prop<boolean>(o, def, "layout", "accent");
  const showLabel = prop<boolean>(o, def, "label", "show");
  return (
    <div className={`multi-card ${accent ? "has-accent" : ""}`} style={columns > 0 ? { gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` } : undefined}>
      {fields.map((p, i) => {
        const raw = data.columns.get(`values:${i}`)?.get(0);
        const f = projectionFormat(model, p);
        const text = raw === null || raw === undefined ? t("(Blank)") : typeof raw === "number" && f.type !== "String" ? withUnits(raw, units, f, locale) : fmt(raw, f, locale);
        return (
          <div key={i} className="multi-card-item" data-callout={i}>
            <div className={`card-value tabular ${raw == null ? "is-blank" : ""}`} style={{ fontSize: size }} title={raw == null ? undefined : fmt(raw, f, locale)}>
              {text}
            </div>
            {showLabel && <div className="card-label">{projectionName(model, p, t)}</div>}
          </div>
        );
      })}
    </div>
  );
}
