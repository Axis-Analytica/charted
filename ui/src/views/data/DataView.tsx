import type { Rectangle } from "@glideapps/glide-data-grid";
import { ChartNoAxesColumn, ChevronDown, ChevronRight, Code2, FunnelX, PanelRight, Search, SquareFunction, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "../../bridge";
import type { Column } from "../../bridge/generated/Column";
import type { ProjectSnapshot } from "../../bridge/generated/ProjectSnapshot";
import type { Table } from "../../bridge/generated/Table";
import type { ViewCount } from "../../bridge/generated/ViewCount";
import { useShortcut } from "../../app/shortcuts";
import { formatValue } from "../../format";
import { formatNumber, useLanguage, useT } from "../../i18n";
import { Button, IconButton } from "../../kit/Button";
import { Banner, Spinner } from "../../kit/misc";
import { dataJson, nextSort, useDataView, viewSpec } from "../../state/dataview";
import { toast } from "../../state/toasts";
import { useViewAs, viewedRoles } from "../../state/viewas";
import { fetchRows } from "./blocks";
import { ColumnFormulaBar } from "./ColumnFormulaBar";
import { ColumnProperties } from "./ColumnProperties";
import { DataGrid, type GridHandle } from "./DataGrid";
import { FilterMenu } from "./FilterMenu";
import { FindBar } from "./FindBar";
import { StatsDrawer } from "./StatsDrawer";
import "./data.css";
import { Hideable } from "../../app/Hideable";

const MAX_COPY_ROWS = 1_000_000;

function tsvCell(s: string): string {
  return /[\t\n\r"]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** Puts text on the clipboard. The text may still be loading: a ClipboardItem holding a
 * promise keeps the keypress's permission to write while the rows arrive. */
async function copyToClipboard(text: Promise<string>): Promise<void> {
  if (typeof ClipboardItem !== "undefined" && navigator.clipboard?.write) {
    try {
      await navigator.clipboard.write([new ClipboardItem({ "text/plain": text.then((s) => new Blob([s], { type: "text/plain" })) })]);
      return;
    } catch {
      // Fall through to the plain API (older engines reject promised items).
    }
  }
  const s = await text;
  if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(s);
  const area = document.createElement("textarea");
  area.value = s;
  document.body.append(area);
  area.select();
  document.execCommand("copy");
  area.remove();
}

export function DataView({ project, table }: { project: ProjectSnapshot; table: Table }) {
  const { t, tn } = useT();
  const locale = useLanguage((s) => s.locale);
  const store = useDataView();
  const tv = store.view(table.id);
  const spec = useMemo(() => viewSpec(table.id, tv), [table.id, tv]);
  const selectedId = store.selected[table.id];
  const columns: Column[] = table.columns;
  const selected = columns.find((c) => c.id === selectedId);
  // Calculated columns' values change with their expression.
  const dataKey = columns.map((c) => `${c.id}:${c.dataType}:${c.sortBy ?? ""}:${c.kind.kind === "calculated" ? c.kind.expression : ""}`).join("|");
  // The formula bar: a new column, or the selected calculated column.
  const [newColumn, setNewColumn] = useState(false);
  const goTo = useDataView((s) => s.goTo);
  useEffect(() => {
    if (goTo?.table !== table.id) return;
    if (goTo.newColumn) setNewColumn(true);
    useDataView.getState().set({ goTo: null });
  }, [goTo, table.id]);
  // A new column is shown once the grid has it.
  const scrollTo = useRef<string | null>(null);
  useEffect(() => {
    const id = scrollTo.current;
    if (id && columns.some((c) => c.id === id)) {
      scrollTo.current = null;
      requestAnimationFrame(() => grid.current?.goTo(id, 0));
    }
  }, [columns]);
  const formulaColumn = newColumn ? null : selected?.kind.kind === "calculated" ? selected : undefined;
  const total = Number(project.tables.find((s) => s.id === table.id)?.rows ?? 0);
  const viewing = useViewAs((s) => viewedRoles(s, project.path));
  const query = project.queries.find((q) => q.table === table.source);

  const grid = useRef<GridHandle>(null);
  // Times each new view from its request to its first painted rows (the HUD shows it).
  const started = useRef(performance.now());
  const viewKey = JSON.stringify(spec);
  useEffect(() => {
    started.current = performance.now();
  }, [viewKey]);
  const [rows, setRows] = useState<number | null>(spec.filters.length ? null : total);
  const [counting, setCounting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ column: string; bounds: Rectangle } | null>(null);
  const [showSql, setShowSql] = useState(false);

  // Row count of the view (filters change it; sorting does not).
  const filterKey = JSON.stringify(spec.filters) + dataKey + total;
  useEffect(() => {
    let live = true;
    setCounting(true);
    dataJson<ViewCount>({ type: "count", view: { ...spec, sort: [] }, column: null })
      .then((c) => {
        if (!live) return;
        setRows(c.rows);
        setError(null);
      })
      .catch((e) => live && setError(errorMessage(e)))
      .finally(() => live && setCounting(false));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [table.id, filterKey]);

  // Distinct count of the selected column, for the status bar.
  useEffect(() => {
    let live = true;
    store.set({ counts: { table: table.id, rows, distinct: null } });
    if (!selectedId || rows === null) return;
    dataJson<ViewCount>({ type: "count", view: { ...spec, sort: [] }, column: selectedId })
      .then((c) => live && useDataView.getState().set({ counts: { table: table.id, rows: c.rows, distinct: c.distinct } }))
      .catch(() => {});
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [table.id, filterKey, selectedId, rows]);
  useEffect(() => () => useDataView.getState().set({ counts: null }), []);

  const sortBy = useCallback((column: string, add: boolean) => store.setSort(table.id, nextSort(useDataView.getState().view(table.id).sort, column, add)), [store, table.id]);

  const copy = useCallback(() => {
    const sel = grid.current?.selection();
    if (!sel || !sel.columns.length || rows === null) return;
    const count = Math.min(sel.last - sel.first + 1, MAX_COPY_ROWS);
    const cols = sel.columns.map((id) => columns.find((c) => c.id === id)!).filter(Boolean);
    const whole = sel.first === 0 && sel.last === rows - 1 && rows > 1;
    const text = fetchRows(spec, sel.columns, sel.first, count).then((data) => {
      const vectors = cols.map((c) => data.getChild(c.id));
      const lines: string[] = [];
      if (whole || count > 1) lines.push(cols.map((c) => tsvCell(c.name)).join("\t"));
      for (let r = 0; r < data.numRows; r++) lines.push(cols.map((c, i) => tsvCell(formatValue(vectors[i]?.get(r), c.dataType, c.formatString, locale))).join("\t"));
      return lines.join("\n");
    });
    copyToClipboard(text)
      .then(() => {
        const n = sel.last - sel.first + 1;
        if (n > MAX_COPY_ROWS) toast({ kind: "info", title: t("Copied the first %1 rows", formatNumber(MAX_COPY_ROWS)) });
        else toast({ kind: "success", title: count === 1 && cols.length === 1 ? t("Copied") : tn("Copied %n rows", count, "Copied %n row") });
      })
      .catch((e) => toast({ kind: "error", title: t("Could not copy: %1", errorMessage(e)) }));
  }, [columns, rows, spec, locale, t, tn]);

  useShortcut(
    { id: "copy", keys: "Ctrl+C", label: "Copy selection as TSV", group: "Edit", run: copy, enabled: () => !window.getSelection()?.toString() },
    { id: "new-column", keys: "", label: "New column", group: "Edit", run: () => setNewColumn(true) },
    { id: "stats", keys: "Ctrl+I", label: "Column statistics", group: "View", run: () => store.set({ statsOpen: !useDataView.getState().statsOpen }) },
    { id: "find", keys: "Ctrl+F", label: "Find in table", group: "Edit", run: () => store.set({ findOpen: true }), global: true },
    { id: "props", keys: "Ctrl+Alt+Enter", label: "Column properties", group: "View", run: () => store.set({ propsOpen: !useDataView.getState().propsOpen }) },
    {
      id: "column-menu",
      keys: "Alt+ArrowDown",
      label: "Sort and filter the selected column",
      group: "Edit",
      run: () => {
        const id = useDataView.getState().selected[table.id];
        const bounds = id && grid.current?.headerBounds(id);
        if (id && bounds) setMenu({ column: id, bounds });
      },
      enabled: () => !!useDataView.getState().selected[table.id],
    },
    { id: "clear-filters", keys: "Ctrl+Shift+L", label: "Clear all filters", group: "Edit", run: () => store.clearFilters(table.id), enabled: () => useDataView.getState().view(table.id).filters.length > 0 },
  );

  const menuColumn = menu && columns.find((c) => c.id === menu.column);
  const nameOf = (id: string) => columns.find((c) => c.id === id)?.name ?? id;
  const filtered = rows !== null && spec.filters.length > 0;

  return (
    <div className="dataview">
      <div className="preview-head">
        <div>
          <h2 className="preview-title">{table.name}</h2>
          <div className="preview-sub">
            {filtered ? t("%1 of %2 rows", formatNumber(rows), formatNumber(total)) : tn("%n rows", total, "%n row")} · {tn("%n columns", columns.length, "%n column")}
            {query && (
              <>
                {" · "}
                <span className="mono">{query.sourcePath}</span>
              </>
            )}
            {counting && <Spinner size={11} />}
          </div>
        </div>
        <div className="dv-tools">
          <Hideable id="data-new-column">
            <Button size="sm" variant="ghost" icon={<SquareFunction size={14} />} onPress={() => setNewColumn(true)}>
              {t("New column")}
            </Button>
          </Hideable>
          <Hideable id="data-find">
            <IconButton label={t("Find in table")} shortcut="Ctrl+F" icon={<Search size={15} />} onPress={() => store.set({ findOpen: true })} />
          </Hideable>
          <Hideable id="data-stats">
            <IconButton label={t("Column statistics")} shortcut="Ctrl+I" icon={<ChartNoAxesColumn size={15} />} pressed={store.statsOpen} onPress={() => store.set({ statsOpen: !store.statsOpen })} />
          </Hideable>
          <Hideable id="data-props">
            <IconButton label={t("Column properties")} shortcut="Ctrl+Alt+Enter" icon={<PanelRight size={15} />} pressed={store.propsOpen} onPress={() => store.set({ propsOpen: !store.propsOpen })} />
          </Hideable>
          {query && (
            <button type="button" className="sql-toggle" aria-expanded={showSql} onClick={() => setShowSql((s) => !s)}>
              <Code2 size={14} />
              {t("Load query")}
              {showSql ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
            </button>
          )}
        </div>
      </div>
      {showSql && query && <pre className="sql-panel mono">{query.sql}</pre>}
      {formulaColumn !== undefined && (
        <ColumnFormulaBar
          model={project.model}
          table={table}
          column={formulaColumn}
          onDone={() => {
            setNewColumn(false);
            if (formulaColumn) store.select(table.id, undefined);
          }}
          onCreated={(id) => {
            setNewColumn(false);
            store.select(table.id, id);
            scrollTo.current = id;
          }}
        />
      )}
      {store.findOpen && <FindBar view={spec} onFound={(r) => grid.current?.goTo(r.column, r.row)} onClose={() => (store.set({ findOpen: false }), grid.current?.focus())} />}
      {(spec.filters.length > 0 || spec.sort.length > 0) && (
        <div className="chips" aria-label={t("Sort and filters")}>
          {spec.sort.map((k, i) => (
            <span key={`s${k.column}`} className="chip">
              <span className="chip-kind">{i === 0 ? t("Sorted by") : t("then")}</span>
              {nameOf(k.column)} {k.descending ? "↓" : "↑"}
              <button type="button" className="chip-x" aria-label={t("Remove sort by %1", nameOf(k.column))} onClick={() => store.setSort(table.id, spec.sort.filter((x) => x.column !== k.column))}>
                <X size={11} />
              </button>
            </span>
          ))}
          {spec.filters.map((f) => (
            <span key={`f${f.column}`} className="chip is-filter">
              <button type="button" className="chip-body" onClick={(e) => setMenu({ column: f.column, bounds: (e.currentTarget as HTMLElement).getBoundingClientRect() })}>
                {nameOf(f.column)}: {describeFilter(f.filter, t)}
              </button>
              <button type="button" className="chip-x" aria-label={t("Remove filter on %1", nameOf(f.column))} onClick={() => store.setFilter(table.id, f.column, null)}>
                <X size={11} />
              </button>
            </span>
          ))}
          {spec.filters.length > 0 && (
            <Button size="sm" variant="ghost" icon={<FunnelX size={13} />} onPress={() => store.clearFilters(table.id)}>
              {t("Clear filters")}
            </Button>
          )}
        </div>
      )}
      {error && <Banner tone="danger">{error}</Banner>}
      {viewing.length > 0 && <Banner tone="info">{t("View as applies to the report: the data view shows every row.")}</Banner>}
      <div className="dv-body">
        <div className="dv-main">
          <div className="preview-grid">
            {rows === null ? (
              <div className="preview-loading">
                <Spinner size={18} />
              </div>
            ) : (
              <DataGrid
                ref={grid}
                columns={columns}
                view={spec}
                dataKey={dataKey}
                rows={rows}
                widths={tv.widths}
                selected={selectedId}
                onSelect={(c) => store.select(table.id, c)}
                onSort={sortBy}
                onMenu={(column, bounds) => setMenu({ column, bounds })}
                onResize={(c, w) => store.setWidth(table.id, c, w)}
                onError={setError}
                onFirstRows={() => store.set({ firstRows: Math.round(performance.now() - started.current) })}
              />
            )}
            {rows === 0 && filtered && (
              <div className="dv-empty">
                <p>{t("No rows match the filters.")}</p>
                <Button size="sm" onPress={() => store.clearFilters(table.id)}>
                  {t("Clear filters")}
                </Button>
              </div>
            )}
          </div>
          {store.statsOpen &&
            (selected ? (
              <StatsDrawer column={selected} view={spec} onClose={() => store.set({ statsOpen: false })} />
            ) : (
              <section className="stats stats-empty">{t("Select a column to see its statistics.")}</section>
            ))}
        </div>
        {store.propsOpen && selected && <ColumnProperties table={table} column={selected} onClose={() => store.set({ propsOpen: false })} />}
      </div>
      {menu && menuColumn && (
        <FilterMenu
          column={menuColumn}
          view={spec}
          anchor={menu.bounds}
          onClose={() => setMenu(null)}
          onSort={(descending, add) => {
            const sort = useDataView.getState().view(table.id).sort;
            const rest = sort.filter((k) => k.column !== menuColumn.id);
            if (descending === null) store.setSort(table.id, rest);
            else if (add) store.setSort(table.id, sort.some((k) => k.column === menuColumn.id) ? sort.map((k) => (k.column === menuColumn.id ? { ...k, descending } : k)) : [...sort, { column: menuColumn.id, descending }]);
            else store.setSort(table.id, [{ column: menuColumn.id, descending }]);
          }}
          onFilter={(f) => store.setFilter(table.id, menuColumn.id, f)}
        />
      )}
    </div>
  );
}

function describeFilter(f: import("../../bridge/generated/Filter").Filter, t: (s: string, ...a: (string | number)[]) => string): string {
  if (f.kind === "values") {
    const shown = f.values.slice(0, 2).map((v) => v ?? t("(Blank)"));
    const more = f.values.length > 2 ? ` +${f.values.length - 2}` : "";
    return `${f.exclude ? t("not") + " " : ""}${shown.join(", ")}${more}`;
  }
  const v = f.value ?? "";
  switch (f.op) {
    case "isBlank": return t("is blank");
    case "isNotBlank": return t("is not blank");
    case "between": return `${v} – ${f.value2 ?? ""}`;
    case "contains": return t("contains %1", v);
    case "notContains": return t("does not contain %1", v);
    case "startsWith": return t("starts with %1", v);
    case "endsWith": return t("ends with %1", v);
    case "equals": return `= ${v}`;
    case "notEquals": return `≠ ${v}`;
    case "less": return `< ${v}`;
    case "lessOrEqual": return `≤ ${v}`;
    case "greater": return `> ${v}`;
    case "greaterOrEqual": return `≥ ${v}`;
  }
}
