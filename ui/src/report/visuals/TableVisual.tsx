import { ArrowDown, ArrowUp } from "lucide-react";
import type { Model } from "../../bridge/generated/Model";
import type { Visual } from "../../bridge/generated/Visual";
import { useLanguage, useT } from "../../i18n";
import { isGrouping, projectionName } from "../../state/report";
import { keysOf, type VisualData } from "../data";
import { prop, visualDef } from "../registry";
import { fmt, projectionFormat } from "./common";
import { additive, type Interact, noInteract } from "./interact";

export function TableVisual({ visual, model, data, onSort, interact = noInteract }: { visual: Visual; model: Model; data: VisualData; onSort(index: number): void; interact?: Interact }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const def = visualDef("table");
  const cols = visual.roles.columns ?? [];
  const formats = cols.map((p) => projectionFormat(model, p));
  const vectors = cols.map((_, i) => data.columns.get(`columns:${i}`));
  const numeric = formats.map((f) => f.type === "Int64" || f.type === "Double" || f.type === "Decimal");
  const fontSize = prop<number>(visual.objects, def, "grid", "fontSize");
  const banded = prop<boolean>(visual.objects, def, "grid", "banded");
  const rowLines = prop<boolean>(visual.objects, def, "grid", "rowLines");
  const totals = prop<boolean>(visual.objects, def, "totals", "show") && data.totals && cols.some((p) => !isGrouping(p));
  const rows = Math.min(data.rows, 5000);
  // A row is chosen by the values of its grouping columns.
  const groups = cols.map((p, i) => ({ p, i })).filter((g) => isGrouping(g.p));
  const keys = groups.map((g) => keysOf(data, `columns:${g.i}`));
  const tupleAt = (r: number) => keys.map((k) => k[r] ?? null);
  return (
    <div className={`table-visual ${banded ? "is-banded" : ""} ${rowLines ? "has-lines" : ""}`} style={{ fontSize }}>
      <table>
        <thead>
          <tr>
            {cols.map((p, i) => {
              const sorted = visual.sort?.role === "columns" && visual.sort.index === i ? visual.sort : null;
              return (
                <th key={i} className={numeric[i] ? "is-num" : ""} aria-sort={sorted ? (sorted.descending ? "descending" : "ascending") : "none"}>
                  <button type="button" onClick={() => onSort(i)}>
                    {projectionName(model, p, t)}
                    {sorted && (sorted.descending ? <ArrowDown size={11} /> : <ArrowUp size={11} />)}
                  </button>
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {Array.from({ length: rows }, (_, r) => {
            const chosen = groups.length ? interact.chosen(tupleAt(r)) : null;
            return (
              <tr
                key={r}
                className={chosen === false ? "is-dim" : chosen ? "is-chosen" : ""}
                onClick={(e) =>
                  groups.length &&
                  interact.choose(
                    groups.map((g) => g.p),
                    tupleAt(r),
                    additive(e),
                  )
                }
              >
                {cols.map((_, i) => (
                  <td key={i} className={numeric[i] ? "is-num tabular" : ""}>
                    {fmt(vectors[i]?.get(r), formats[i]!, locale)}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
        {totals && (
          <tfoot>
            <tr>
              {cols.map((p, i) => (
                <td key={i} className={numeric[i] ? "is-num tabular" : ""}>
                  {i === 0 && isGrouping(p) ? t("Total") : isGrouping(p) ? "" : fmt(data.totals?.get(`columns:${i}`), formats[i]!, locale)}
                </td>
              ))}
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}
