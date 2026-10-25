// The query editor's preview: the rows a step produces, with Power Query's column
// profile above each column (quality: valid, error and empty; distribution: the most
// common values, distinct and unique counts). Each header has a type button and a menu
// of transforms for that column.
import { Calendar, CalendarClock, ChevronDown, Clock, Hash, Percent, ToggleLeft, Type as TypeIcon } from "lucide-react";
import type { ReactNode } from "react";
import type { ColumnProfile } from "../bridge/generated/ColumnProfile";
import type { DataType } from "../bridge/generated/DataType";
import type { QueryColumn } from "../bridge/generated/QueryColumn";
import { useT } from "../i18n";
import { Menu, type MenuEntry } from "../kit/Menu";
import type { Preview } from "./data";
import { TYPES } from "./transforms";

const TYPE_ICON: Partial<Record<DataType, ReactNode>> = {
  String: <TypeIcon size={12} />,
  Int64: <Hash size={12} />,
  Double: <Percent size={12} />,
  Decimal: <Percent size={12} />,
  Date: <Calendar size={12} />,
  DateTime: <CalendarClock size={12} />,
  DateTimeTz: <CalendarClock size={12} />,
  Time: <Clock size={12} />,
  Boolean: <ToggleLeft size={12} />,
};

/** A cell as text. Dates and timestamps arrive as epoch milliseconds; the column's
 * Arrow type says which. */
function cell(v: unknown, arrowType: string): string | null {
  if (v === null || v === undefined) return null;
  const t = arrowType.toLowerCase();
  if (t.startsWith("date") && (typeof v === "number" || typeof v === "bigint")) return new Date(Number(v)).toISOString().slice(0, 10);
  if (t.startsWith("timestamp") && (typeof v === "number" || typeof v === "bigint")) return new Date(Number(v)).toISOString().slice(0, 19).replace("T", " ");
  if (v instanceof Date) return v.toISOString().slice(0, 10);
  if (typeof v === "boolean") return v ? "TRUE" : "FALSE";
  return String(v);
}

const isNumeric = (t: DataType | undefined) => t === "Int64" || t === "Double" || t === "Decimal";

function Quality({ p }: { p: ColumnProfile }) {
  const { t } = useT();
  const n = Math.max(1, p.count);
  const errors = p.errors;
  const empty = p.empty;
  const valid = Math.max(0, p.count - errors - empty);
  const pct = (x: number) => `${Math.round((x / n) * 100)}%`;
  return (
    <div className="qe-quality" title={t("Valid %1 · Error %2 · Empty %3", pct(valid), pct(errors), pct(empty))}>
      <div className="qe-quality-bar">
        <span className="is-valid" style={{ width: pct(valid) }} />
        <span className="is-error" style={{ width: pct(errors) }} />
        <span className="is-empty" style={{ width: pct(empty) }} />
      </div>
      <div className="qe-quality-text">
        <span>{t("Valid %1", pct(valid))}</span>
        {errors > 0 && <span className="is-error">{t("Error %1", pct(errors))}</span>}
        <span>{t("Empty %1", pct(empty))}</span>
      </div>
    </div>
  );
}

function Distribution({ p }: { p: ColumnProfile }) {
  const { t } = useT();
  const max = Math.max(1, ...p.top.map((x) => x.count));
  return (
    <div className="qe-dist">
      <div className="qe-dist-bars" aria-hidden>
        {p.top.map((x, i) => (
          <span key={i} style={{ height: `${Math.max(6, (x.count / max) * 100)}%` }} title={`${x.value ?? "null"}: ${x.count}`} />
        ))}
      </div>
      <div className="qe-dist-text">{t("%1 distinct, %2 unique", p.distinct, p.unique)}</div>
    </div>
  );
}

interface Props {
  preview: Preview;
  columns?: QueryColumn[];
  profile?: ColumnProfile[] | null;
  limit?: number;
  /** Header menu entries for a column (none: a plain table). */
  menu?(column: string): (MenuEntry | null)[];
  onType?(column: string, type: DataType): void;
}

export function PreviewTable({ preview, columns, profile, limit = 1000, menu, onType }: Props) {
  const { t } = useT();
  const types = new Map(columns?.map((c) => [c.name, c]) ?? []);
  const rows = Math.min(preview.rows, limit);
  return (
    <div className="qe-preview">
      <table>
        <thead>
          <tr>
            <th className="qe-rownum" />
            {preview.names.map((name) => {
              const c = types.get(name);
              return (
                <th key={name} className={isNumeric(c?.dataType) ? "is-num" : ""}>
                  <div className="qe-head">
                    {onType ? (
                      <Menu
                        label={t("Change the type of %1", name)}
                        trigger={<span className="qe-type">{TYPE_ICON[c?.dataType ?? "String"] ?? <TypeIcon size={12} />}</span>}
                        items={TYPES.map((x) => ({ id: x.value, label: t(x.label), run: () => onType(name, x.value) }))}
                      />
                    ) : (
                      <span className="qe-type">{TYPE_ICON[c?.dataType ?? "String"]}</span>
                    )}
                    <span className="qe-name" title={c ? `${name} (${c.duckdbType})` : name}>
                      {name}
                    </span>
                    {menu && <Menu label={t("Column options for %1", name)} trigger={<ChevronDown size={13} />} items={menu(name)} />}
                  </div>
                  {profile && (() => {
                    const p = profile.find((x) => x.name === name);
                    return p ? (
                      <div className="qe-profile">
                        <Quality p={p} />
                        <Distribution p={p} />
                      </div>
                    ) : null;
                  })()}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {Array.from({ length: rows }, (_, r) => (
            <tr key={r}>
              <td className="qe-rownum">{r + 1}</td>
              {preview.columns.map((col, i) => {
                const name = preview.names[i]!;
                const v = cell(col.get(r), String(col.type));
                return (
                  <td key={i} className={`${isNumeric(types.get(name)?.dataType) ? "is-num tabular" : ""} ${v === null ? "is-null" : ""}`}>
                    {v ?? "null"}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
