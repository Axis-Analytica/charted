import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { errorMessage } from "../../bridge";
import type { Column } from "../../bridge/generated/Column";
import type { ColumnStats } from "../../bridge/generated/ColumnStats";
import type { ViewSpec } from "../../bridge/generated/ViewSpec";
import { formatValue } from "../../format";
import { formatNumber, useLanguage, useT } from "../../i18n";
import { IconButton } from "../../kit/Button";
import { Spinner } from "../../kit/misc";
import { dataJson } from "../../state/dataview";
import { DataTypeIcon } from "../icons";
import { isNumeric } from "./DataGrid";

function Tile({ label, value, sub, wide }: { label: string; value: string; sub?: string; wide?: boolean }) {
  return (
    <div className={`stat ${wide ? "is-wide" : ""}`}>
      <div className="stat-label">{label}</div>
      <div className="stat-value tabular" title={value}>
        {value}
      </div>
      {sub && <div className="stat-sub tabular">{sub}</div>}
    </div>
  );
}

interface Bar {
  label: string;
  count: number;
}

/** Single-series bars: one accent hue, thin marks with 2px gaps, rounded data ends on
 * the baseline, a tooltip per bar. Values stay in text colours. */
function Bars({ bars, title, horizontal }: { bars: Bar[]; title: string; horizontal?: boolean }) {
  const [hover, setHover] = useState<number | null>(null);
  const max = Math.max(1, ...bars.map((b) => b.count));
  const total = bars.reduce((a, b) => a + b.count, 0) || 1;
  const pct = (n: number) => formatNumber(n / total, { style: "percent", maximumFractionDigits: 1 });
  if (horizontal) {
    return (
      <figure className="topbars" aria-label={title}>
        <figcaption className="stat-label">{title}</figcaption>
        {bars.map((b, i) => (
          <div key={i} className="topbar-row" title={`${b.label}: ${formatNumber(b.count)} (${pct(b.count)})`}>
            <span className="topbar-label">{b.label}</span>
            <span className="topbar-track">
              <span className="topbar-fill" style={{ width: `${(b.count / max) * 100}%` }} />
            </span>
            <span className="topbar-count tabular">{formatNumber(b.count)}</span>
          </div>
        ))}
      </figure>
    );
  }
  return (
    <figure className="histogram" aria-label={title}>
      <figcaption className="stat-label">{title}</figcaption>
      <div className="hist-plot" onMouseLeave={() => setHover(null)}>
        {bars.map((b, i) => (
          <div key={i} className={`hist-slot ${hover === i ? "is-hover" : ""}`} onMouseEnter={() => setHover(i)}>
            <span className="hist-bar" style={{ height: `${b.count ? Math.max(2, (b.count / max) * 100) : 0}%` }} />
          </div>
        ))}
        {hover !== null && bars[hover] && (
          <div className="hist-tip" role="tooltip" style={{ insetInlineStart: `${((hover + 0.5) / bars.length) * 100}%` }}>
            <div>{bars[hover].label}</div>
            <div className="tabular">
              <strong>{formatNumber(bars[hover].count)}</strong> · {pct(bars[hover].count)}
            </div>
          </div>
        )}
      </div>
      <div className="hist-axis">
        <span>{bars[0]?.label.split(" – ")[0]}</span>
        <span>{bars[bars.length - 1]?.label.split(" – ").pop()}</span>
      </div>
    </figure>
  );
}

export function StatsDrawer({ column, view, onClose }: { column: Column; view: ViewSpec; onClose(): void }) {
  const { t } = useT();
  const locale = useLanguage((s) => s.locale);
  const [stats, setStats] = useState<ColumnStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let live = true;
    setLoading(true);
    dataJson<ColumnStats>({ type: "stats", view, column: column.id })
      .then((s) => live && (setStats(s), setError(null)))
      .catch((e) => live && setError(errorMessage(e)))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
    // Re-query when the column's type changes too.
  }, [view, column.id, column.dataType]);

  const numeric = isNumeric(column.dataType);
  const temporal = column.dataType === "Date" || column.dataType === "DateTime" || column.dataType === "DateTimeTz" || column.dataType === "Time";
  const fmt = (v: unknown) => (v === null || v === undefined ? "–" : formatValue(v, column.dataType, column.formatString, locale) || "–");
  // Spread and averages of whole numbers get decimals even when the column has none.
  const fmtStat = (v: number | null) => {
    if (v === null) return "–";
    if (column.formatString && column.dataType !== "Int64") return formatValue(v, column.dataType, column.formatString, locale);
    return formatNumber(v, { maximumFractionDigits: Math.abs(v) >= 100 ? 1 : 2 });
  };
  const n = (v: number) => formatNumber(v);
  const share = (v: number) => (stats && stats.rows ? formatNumber(v / stats.rows, { style: "percent", maximumFractionDigits: 1 }) : "");

  let bars: Bar[] = [];
  if (stats && stats.histogram.length) {
    const label = (x: number) => (temporal ? formatValue(x * 1000, column.dataType, null, locale) : fmtStat(x));
    const unit = column.dataType === "Int64" && stats.histogram.every((b) => b.end - b.start === 1);
    bars = stats.histogram.map((b) => ({ label: unit ? label(b.start) : `${label(b.start)} – ${label(b.end)}`, count: b.count }));
  } else if (stats) {
    bars = stats.top.map((v) => ({ label: v.value === null ? t("(Blank)") : v.value === "" ? t("(Empty)") : column.dataType === "Boolean" ? fmt(v.value === "true") : v.value, count: v.count }));
  }

  return (
    <section className="stats" aria-label={t("Column statistics")}>
      <div className="stats-head">
        <DataTypeIcon type={column.dataType} />
        <span className="stats-title">{column.name}</span>
        <span className="stats-sub">{view.filters.length ? t("of the filtered rows") : t("of all rows")}</span>
        {loading && <Spinner size={12} />}
        <span className="status-fill" />
        <IconButton label={t("Close statistics")} shortcut="Ctrl+I" icon={<X size={14} />} onPress={onClose} />
      </div>
      {error ? (
        <div className="stats-error">{error}</div>
      ) : stats ? (
        <div className={`stats-body ${loading ? "is-stale" : ""}`}>
          <div className="stat-grid">
            <Tile label={t("Values")} value={n(stats.count)} sub={share(stats.count)} />
            <Tile label={t("Distinct")} value={n(stats.distinct)} sub={stats.count ? t("%1 per value", formatNumber(stats.count / Math.max(1, stats.distinct), { maximumFractionDigits: 1 })) : undefined} />
            <Tile label={t("Blank")} value={n(stats.nulls)} sub={share(stats.nulls)} />
            {numeric && <Tile label={t("Zeros")} value={n(stats.empty)} sub={share(stats.empty)} />}
            {column.dataType === "String" && <Tile label={t("Empty")} value={n(stats.empty)} sub={share(stats.empty)} />}
            <Tile label={t("Minimum")} value={fmt(stats.min)} />
            <Tile label={t("Maximum")} value={fmt(stats.max)} />
            {numeric && (
              <>
                <Tile label={t("Mean")} value={fmtStat(stats.mean)} />
                <Tile label={t("Median")} value={fmtStat(stats.median)} />
                <Tile label={t("Std. deviation")} value={fmtStat(stats.stddev)} />
                <Tile wide label={t("25th–75th percentile")} value={`${fmtStat(stats.p25)} – ${fmtStat(stats.p75)}`} />
              </>
            )}
            {stats.spanDays !== null && <Tile label={t("Span")} value={t("%1 days", n(stats.spanDays))} />}
            {stats.minLength !== null && <Tile label={t("Length")} value={`${n(stats.minLength)} – ${n(stats.maxLength ?? 0)}`} sub={t("characters")} />}
          </div>
          {bars.length > 0 && <Bars bars={bars} title={stats.histogram.length ? t("Distribution") : t("Most common values")} horizontal={!stats.histogram.length} />}
        </div>
      ) : null}
    </section>
  );
}
