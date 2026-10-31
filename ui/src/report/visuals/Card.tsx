import { fontStack } from "../../design/fontNames";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage } from "../../i18n";
import { projectionName } from "../../state/report";
import type { VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { projectionFormat, withUnits, fmt } from "./common";

export function Card({ visual, model, data }: { visual: Visual; model: Model; data: VisualData }) {
  const locale = useLanguage((s) => s.locale);
  const def = visualDef("card");
  const p = visual.roles.values?.[0];
  if (!p) return null;
  const raw = data.columns.get("values:0")?.get(0);
  const f = projectionFormat(model, p);
  const units = prop<string>(visual.objects, def, "callout", "displayUnits");
  const size = prop<number>(visual.objects, def, "callout", "fontSize");
  const text = raw === null || raw === undefined ? "(Blank)" : typeof raw === "number" && f.type !== "String" ? withUnits(raw, units, f, locale) : fmt(raw, f, locale);
  return (
    <div className="card-visual">
      <div className={`card-value tabular ${raw == null ? "is-blank" : ""}`} style={{ fontSize: size, fontFamily: fontStack(prop<string>(visual.objects, def, "callout", "fontFamily")) ?? "var(--report-callout-font, inherit)" }} title={raw == null ? undefined : fmt(raw, f, locale)}>
        {text}
      </div>
      {prop<boolean>(visual.objects, def, "label", "show") && <div className="card-label">{projectionName(model, p)}</div>}
    </div>
  );
}
