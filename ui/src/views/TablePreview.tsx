import "@glideapps/glide-data-grid/dist/index.css";
import { DataEditor, type GridCell, GridCellKind, type GridColumn, type Item, type Theme as GridTheme } from "@glideapps/glide-data-grid";
import { tableFromIPC, type Table as ArrowTable } from "apache-arrow";
import { ChevronDown, ChevronRight, Code2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { Column } from "../bridge/generated/Column";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import type { Table } from "../bridge/generated/Table";
import { formatValue } from "../format";
import { useLanguage, useT } from "../i18n";
import { Banner, Spinner } from "../kit/misc";
import { useApp } from "../state/app";

const PREVIEW_ROWS = 200;

/** Glide's theme, read from the live design tokens. */
function useGridTheme(): Partial<GridTheme> {
  const env = useApp((s) => s.env);
  return useMemo(() => {
    const v = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return {
      accentColor: v("--accent"),
      accentFg: v("--accent-fg"),
      accentLight: v("--accent-soft"),
      textDark: v("--fg"),
      textMedium: v("--fg-muted"),
      textLight: v("--fg-faint"),
      textHeader: v("--fg-muted"),
      textHeaderSelected: v("--accent-fg"),
      bgCell: v("--bg"),
      bgCellMedium: v("--bg-raised"),
      bgHeader: v("--bg-raised"),
      bgHeaderHasFocus: v("--bg-active"),
      bgHeaderHovered: v("--bg-hover"),
      bgBubble: v("--bg-raised"),
      bgSearchResult: v("--accent-soft"),
      borderColor: v("--border"),
      horizontalBorderColor: v("--border"),
      drilldownBorder: v("--border"),
      linkColor: v("--accent-text"),
      headerFontStyle: "600 12px",
      baseFontStyle: "13px",
      fontFamily: v("--font-ui") || "Inter Variable, system-ui",
      editorFontSize: "13px",
      cellHorizontalPadding: 10,
      cellVerticalPadding: 4,
    };
    // Recompute whenever the environment (theme) changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [env]);
}

export function TablePreview({ project, table }: { project: ProjectSnapshot; table: Table }) {
  const { t, tn } = useT();
  const locale = useLanguage((s) => s.locale);
  const theme = useGridTheme();
  const [data, setData] = useState<ArrowTable | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showSql, setShowSql] = useState(false);
  const rows = Number(project.tables.find((s) => s.id === table.id)?.rows ?? 0);
  const query = project.queries.find((q) => q.table === table.source);

  useEffect(() => {
    let live = true;
    setData(null);
    setError(null);
    transport
      .data({ type: "preview", table: table.id, limit: PREVIEW_ROWS })
      .then((buf) => live && setData(tableFromIPC(new Uint8Array(buf))))
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [table.id]);

  const fields = useMemo(() => {
    if (!data) return [];
    return data.schema.fields.map((f, i) => {
      const column: Column | undefined = table.columns.find((c) => c.kind.kind === "data" && c.kind.sourceColumn === f.name) ?? table.columns.find((c) => c.name === f.name);
      return { name: f.name, index: i, column };
    });
  }, [data, table]);

  const columns: GridColumn[] = useMemo(
    () => fields.map((f) => ({ id: f.name, title: f.column?.name ?? f.name, width: Math.min(260, Math.max(90, (f.column?.name ?? f.name).length * 9 + 40)) })),
    [fields],
  );

  const getCellContent = useCallback(
    ([col, row]: Item): GridCell => {
      const f = fields[col];
      const vector = f && data?.getChildAt(f.index);
      const raw = vector?.get(row);
      const type = f?.column?.dataType ?? "Unknown";
      const text = formatValue(typeof raw === "bigint" ? Number(raw) : raw, type, f?.column?.formatString, locale);
      const numeric = type === "Int64" || type === "Double" || type === "Decimal";
      return { kind: GridCellKind.Text, data: text, displayData: text, allowOverlay: false, contentAlign: numeric ? "right" : "left" };
    },
    [fields, data, locale],
  );

  return (
    <div className="preview">
      <div className="preview-head">
        <div>
          <h2 className="preview-title">{table.name}</h2>
          <div className="preview-sub">
            {tn("%n rows", rows, "%n row")} · {tn("%n columns", table.columns.length, "%n column")}
            {query && (
              <>
                {" · "}
                <span className="mono">{query.sourcePath}</span>
              </>
            )}
            {rows > PREVIEW_ROWS && <> · {t("showing the first %1", PREVIEW_ROWS)}</>}
          </div>
        </div>
        {query && (
          <button type="button" className="sql-toggle" aria-expanded={showSql} onClick={() => setShowSql((s) => !s)}>
            <Code2 size={14} />
            {t("Load query")}
            {showSql ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
          </button>
        )}
      </div>
      {showSql && query && <pre className="sql-panel mono">{query.sql}</pre>}
      {error && <Banner tone="danger">{error}</Banner>}
      <div className="preview-grid">
        {!data && !error ? (
          <div className="preview-loading">
            <Spinner size={18} />
          </div>
        ) : data ? (
          <DataEditor
            columns={columns}
            rows={data.numRows}
            getCellContent={getCellContent}
            theme={theme}
            rowMarkers="number"
            smoothScrollX
            smoothScrollY
            width="100%"
            height="100%"
            headerHeight={32}
            rowHeight={28}
            getCellsForSelection
          />
        ) : null}
      </div>
    </div>
  );
}
