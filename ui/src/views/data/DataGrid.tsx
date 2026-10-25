import "@glideapps/glide-data-grid/dist/index.css";
import {
  CompactSelection,
  DataEditor,
  type DataEditorRef,
  type DrawHeaderCallback,
  type GridCell,
  GridCellKind,
  type GridColumn,
  type GridSelection,
  type Item,
  type Rectangle,
  type SpriteMap,
  type Theme as GridTheme,
} from "@glideapps/glide-data-grid";
import { Binary, Calendar, CalendarClock, Clock, Hash, Percent, ToggleLeft, Type } from "lucide-react";
import { createElement, forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { Column } from "../../bridge/generated/Column";
import type { DataType } from "../../bridge/generated/DataType";
import type { SortKey } from "../../bridge/generated/SortKey";
import type { ViewSpec } from "../../bridge/generated/ViewSpec";
import { formatValue } from "../../format";
import { useLanguage } from "../../i18n";
import { useApp } from "../../state/app";
import { BlockCache } from "./blocks";

export const isNumeric = (t: DataType) => t === "Int64" || t === "Double" || t === "Decimal";

const TYPE_ICONS: Record<DataType, typeof Type> = {
  String: Type,
  Int64: Hash,
  Double: Percent,
  Decimal: Percent,
  Date: Calendar,
  DateTime: CalendarClock,
  DateTimeTz: CalendarClock,
  Time: Clock,
  Boolean: ToggleLeft,
  Binary: Binary,
  Unknown: Binary,
};

/** Header icons drawn from the same Lucide glyphs as the Fields pane. */
const headerIcons: SpriteMap = Object.fromEntries(
  Object.entries(TYPE_ICONS).map(([type, icon]) => [
    `type-${type}`,
    ({ fgColor }: { fgColor: string }) => renderToStaticMarkup(createElement(icon, { color: fgColor, size: 20, strokeWidth: 2, absoluteStrokeWidth: true })),
  ]),
);

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
      bgIconHeader: "transparent",
      fgIconHeader: v("--fg-faint"),
      borderColor: v("--border"),
      horizontalBorderColor: v("--border"),
      drilldownBorder: v("--border"),
      linkColor: v("--accent-text"),
      headerFontStyle: "600 12px",
      baseFontStyle: "13px",
      fontFamily: v("--font-ui") || "Inter Variable, system-ui",
      editorFontSize: "13px",
      headerIconSize: 15,
      cellHorizontalPadding: 10,
      cellVerticalPadding: 4,
    };
    // Recompute whenever the environment (theme) changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [env]);
}

export interface GridHandle {
  /** Scrolls to a cell and selects it. */
  goTo(column: string, row: number): void;
  /** The current selection as column ids and a row range, for copying. */
  selection(): { columns: string[]; first: number; last: number } | null;
  focus(): void;
  /** Screen bounds of a column's header. */
  headerBounds(column: string): Rectangle | undefined;
}

interface Props {
  columns: Column[];
  view: ViewSpec;
  /** Changes when the server-side meaning of the rows changes (types, sort-by). */
  dataKey: string;
  rows: number;
  widths: Record<string, number>;
  selected: string | undefined;
  onSelect(column: string | undefined): void;
  onSort(column: string, add: boolean): void;
  onMenu(column: string, bounds: Rectangle): void;
  onResize(column: string, width: number): void;
  onError(message: string | null): void;
  /** The first block of a new view has painted. */
  onFirstRows?(): void;
}

function defaultWidth(c: Column): number {
  const text = c.name.length * 7.5 + 64;
  const byType = c.dataType === "String" ? 150 : c.dataType === "DateTime" || c.dataType === "DateTimeTz" ? 170 : 110;
  return Math.round(Math.min(320, Math.max(byType, text)));
}

export const DataGrid = forwardRef<GridHandle, Props>(function DataGrid(p, ref) {
  const locale = useLanguage((s) => s.locale);
  const theme = useGridTheme();
  const grid = useRef<DataEditorRef>(null);
  const visible = useRef<Rectangle>({ x: 0, y: 0, width: 0, height: 0 });
  const [selection, setSelection] = useState<GridSelection>({ columns: CompactSelection.empty(), rows: CompactSelection.empty() });
  const columnsRef = useRef(p.columns);
  columnsRef.current = p.columns;
  const rowsRef = useRef(p.rows);
  rowsRef.current = p.rows;

  const viewKey = JSON.stringify(p.view) + p.dataKey;
  const firstRows = useRef(p.onFirstRows);
  firstRows.current = p.onFirstRows;
  const cache = useMemo(
    () => {
      let first = true;
      return new BlockCache(
        p.view,
        (firstRow, last) => {
          // Repaint only the visible cells of the block that arrived.
          const r = visible.current;
          const cells: { cell: Item }[] = [];
          for (let row = Math.max(firstRow, r.y); row <= Math.min(last, r.y + r.height); row++)
            for (let col = r.x; col <= Math.min(r.x + r.width, columnsRef.current.length - 1); col++) cells.push({ cell: [col, row] });
          grid.current?.updateCells(cells);
          p.onError(null);
          if (first) {
            first = false;
            requestAnimationFrame(() => firstRows.current?.());
          }
        },
        (m) => p.onError(m),
      );
    },
    // A new cache per view; callbacks read through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [viewKey],
  );
  useEffect(() => {
    cache.open();
    return () => cache.close();
  }, [cache]);
  useEffect(() => {
    const r = visible.current;
    cache.want(r.y, r.y + Math.max(r.height, 40), p.rows);
  }, [cache, p.rows]);

  const sortOf = useMemo(() => new Map(p.view.sort.map((k: SortKey, i) => [k.column, { i, desc: k.descending }])), [p.view.sort]);
  const filtered = useMemo(() => new Set(p.view.filters.map((f) => f.column)), [p.view.filters]);

  const gridColumns: GridColumn[] = useMemo(
    () =>
      p.columns.map((c) => ({
        id: c.id,
        title: c.name,
        width: p.widths[c.id] ?? defaultWidth(c),
        icon: `type-${c.dataType}`,
        hasMenu: true,
        themeOverride: c.hidden ? { textHeader: theme.textLight, textDark: theme.textMedium } : undefined,
      })),
    [p.columns, p.widths, theme],
  );

  const getCellContent = useCallback(
    ([col, row]: Item): GridCell => {
      const c = columnsRef.current[col];
      const hit = c && cache.cell(c.id, row);
      if (!c || !hit) return { kind: GridCellKind.Loading, allowOverlay: false, skeletonWidth: 50, skeletonWidthVariability: 40 };
      const text = formatValue(hit.vector.get(hit.index), c.dataType, c.formatString, locale);
      return { kind: GridCellKind.Text, data: text, displayData: text, allowOverlay: false, contentAlign: isNumeric(c.dataType) ? "right" : "left" };
    },
    [cache, locale],
  );

  /** Sort arrows (with priority when several) and a funnel on filtered columns. */
  const drawHeader: DrawHeaderCallback = useCallback(
    (args, drawContent) => {
      const { ctx, column, rect, theme: th, isHovered, isSelected, menuBounds, spriteManager } = args;
      const id = column.id ?? "";
      const sort = sortOf.get(id);
      const isFiltered = filtered.has(id);
      if (!sort && !isFiltered) return drawContent();
      const pad = th.cellHorizontalPadding;
      const mid = rect.y + rect.height / 2;
      let x = rect.x + pad;
      if (column.icon) {
        const s = th.headerIconSize;
        spriteManager.drawSprite(column.icon, isSelected ? "selected" : "normal", ctx, x, mid - s / 2, s, th);
        x += Math.ceil(s * 1.3);
      }
      const ink = isSelected ? th.textHeaderSelected : th.accentColor;
      let right = isHovered ? menuBounds.x : rect.x + rect.width - pad;
      ctx.save();
      ctx.strokeStyle = ink;
      ctx.fillStyle = ink;
      ctx.lineWidth = 1.5;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      if (sort) {
        if (p.view.sort.length > 1) {
          ctx.font = `600 10px ${th.fontFamily}`;
          const label = String(sort.i + 1);
          const w = ctx.measureText(label).width;
          ctx.fillText(label, right - w, mid + 4);
          right -= w + 2;
        }
        const ax = right - 4;
        const [tip, tail] = sort.desc ? [mid + 5, mid - 5] : [mid - 5, mid + 5];
        ctx.beginPath();
        ctx.moveTo(ax, tail);
        ctx.lineTo(ax, tip);
        ctx.moveTo(ax - 3.5, tip + (sort.desc ? -3.5 : 3.5));
        ctx.lineTo(ax, tip);
        ctx.lineTo(ax + 3.5, tip + (sort.desc ? -3.5 : 3.5));
        ctx.stroke();
        right -= 13;
      }
      if (isFiltered) {
        const fx = right - 10;
        ctx.beginPath();
        ctx.moveTo(fx, mid - 5);
        ctx.lineTo(fx + 10, mid - 5);
        ctx.lineTo(fx + 6, mid);
        ctx.lineTo(fx + 6, mid + 5);
        ctx.lineTo(fx + 4, mid + 4);
        ctx.lineTo(fx + 4, mid);
        ctx.closePath();
        ctx.fill();
        right -= 15;
      }
      if (isHovered) {
        ctx.strokeStyle = isSelected ? th.textHeaderSelected : th.textHeader;
        const cx = menuBounds.x + menuBounds.width / 2;
        ctx.beginPath();
        ctx.moveTo(cx - 4, mid - 2);
        ctx.lineTo(cx, mid + 2);
        ctx.lineTo(cx + 4, mid - 2);
        ctx.stroke();
      }
      // The title, cut with an ellipsis before the indicators.
      ctx.font = `${th.headerFontStyle} ${th.fontFamily}`;
      ctx.fillStyle = isSelected ? th.textHeaderSelected : th.textHeader;
      let title = column.title;
      const room = right - x - 4;
      if (ctx.measureText(title).width > room) {
        while (title.length > 1 && ctx.measureText(`${title}…`).width > room) title = title.slice(0, -1);
        title += "…";
      }
      ctx.textBaseline = "middle";
      ctx.fillText(title, x, mid);
      ctx.restore();
    },
    [sortOf, filtered, p.view.sort.length],
  );

  const onSelectionChange = (s: GridSelection) => {
    setSelection(s);
    const col = s.columns.first() ?? s.current?.cell[0];
    p.onSelect(col === undefined ? undefined : columnsRef.current[col]?.id);
  };

  // Reflect an outside selection (Fields pane click) in the grid.
  useEffect(() => {
    const i = p.columns.findIndex((c) => c.id === p.selected);
    const cur = selection.columns.first() ?? selection.current?.cell[0];
    if (i >= 0 && cur !== i) {
      setSelection({ columns: CompactSelection.fromSingleSelection(i), rows: CompactSelection.empty() });
      grid.current?.scrollTo(i, visible.current.y, "horizontal");
    }
    // Only when the selection is changed from outside.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p.selected]);

  useImperativeHandle(ref, () => ({
    goTo(column, row) {
      const col = columnsRef.current.findIndex((c) => c.id === column);
      if (col < 0) return;
      setSelection({
        current: { cell: [col, row], range: { x: col, y: row, width: 1, height: 1 }, rangeStack: [] },
        columns: CompactSelection.empty(),
        rows: CompactSelection.empty(),
      });
      grid.current?.scrollTo(col, row, "both", 0, 0, { vAlign: "center" });
      p.onSelect(column);
    },
    selection() {
      const cols = columnsRef.current;
      if (selection.columns.length > 0) {
        const ids = cols.filter((_, i) => selection.columns.hasIndex(i)).map((c) => c.id);
        return { columns: ids, first: 0, last: rowsRef.current - 1 };
      }
      if (selection.rows.length > 0) {
        return { columns: cols.map((c) => c.id), first: selection.rows.first()!, last: selection.rows.last()! };
      }
      const r = selection.current?.range;
      if (!r) return null;
      return { columns: cols.slice(r.x, r.x + r.width).map((c) => c.id), first: r.y, last: r.y + r.height - 1 };
    },
    focus: () => grid.current?.focus(),
    headerBounds(column) {
      const col = columnsRef.current.findIndex((c) => c.id === column);
      return col < 0 ? undefined : grid.current?.getBounds(col, -1);
    },
  }));

  return (
    <div className="data-grid">
      <DataEditor
        ref={grid}
        columns={gridColumns}
        rows={p.rows}
        getCellContent={getCellContent}
        theme={theme}
        headerIcons={headerIcons}
        drawHeader={drawHeader}
        rowMarkers="number"
        smoothScrollX
        smoothScrollY
        width="100%"
        height="100%"
        headerHeight={34}
        rowHeight={28}
        gridSelection={selection}
        onGridSelectionChange={onSelectionChange}
        columnSelect="multi"
        rangeSelect="multi-rect"
        keybindings={{ search: false, copy: false, paste: false, cut: false, downFill: false, rightFill: false, clear: false, delete: false }}
        onHeaderClicked={(col, e) => {
          const c = columnsRef.current[col];
          if (c && e.shiftKey) {
            e.preventDefault();
            p.onSort(c.id, true);
          }
        }}
        onHeaderMenuClick={(col, bounds) => {
          const c = columnsRef.current[col];
          if (c) p.onMenu(c.id, bounds);
        }}
        onHeaderContextMenu={(col, e) => {
          e.preventDefault();
          const c = columnsRef.current[col];
          if (c) p.onMenu(c.id, e.bounds);
        }}
        onColumnResize={(col, size) => col.id && p.onResize(col.id, size)}
        onVisibleRegionChanged={(range) => {
          visible.current = range;
          cache.want(range.y, range.y + range.height, rowsRef.current);
        }}
      />
    </div>
  );
});
