// The Fields pane: tables with their measures and columns. Fields can be dragged to the
// canvas, a visual, a field well or the filters pane; ticking one adds it to the
// selected visual (or creates one), and Enter does the same from the keyboard.
// Right-click (or Shift+F10) on a table, a field or the space around them opens a menu
// to add measures and columns, rename (F2), hide and delete.
import { CalendarDays, ChevronRight, ChevronsDownUp, ChevronsUpDown, Database, Eye, EyeOff, Pencil, Plus, Search, Settings2, Sparkles, SquareFunction, Table2, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Table } from "../bridge/generated/Table";
import { ContextMenu, type ContextMenuState } from "../kit/ContextMenu";
import type { MenuEntry } from "../kit/Menu";
import { useDataView } from "../state/dataview";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { useApp } from "../state/app";
import type { Visual } from "../bridge/generated/Visual";
import { editVisual, fieldKey, newVisual, sameField, useReport, withField, withoutField } from "../state/report";
import { ColumnIcon, MeasureIcon } from "../views/icons";
import { FIELD_MIME } from "./Canvas";

export function FieldsPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const edit = useApp((s) => s.edit);
  const { selected, select, field: current, set } = useReport();
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState<Set<string>>(() => new Set(model.tables.slice(0, 1).map((x) => x.id)));
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  /** What is being renamed: `t:<table id>` or a field's key. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const visual = selected.length === 1 ? page.visuals.find((v) => v.id === selected[0]) : undefined;

  /** Where a field is used in the selected visual. */
  const used = useMemo(() => {
    const m = new Map<string, { role: string; index: number }>();
    if (visual) for (const [role, ps] of Object.entries(visual.roles)) (ps ?? []).forEach((p, index) => m.set(fieldKey(p.field), { role, index }));
    return m;
  }, [visual]);

  const toggleField = async (field: FieldRef) => {
    if (visual) {
      // Decide from the latest state: an earlier tick may still be landing.
      const findUse = (v: Visual) => Object.entries(v.roles).flatMap(([role, ps]) => (ps ?? []).map((p, index) => ({ role, index, p }))).find((x) => fieldKey(x.p.field) === fieldKey(field));
      await edit(
        "Change fields",
        editVisual(page.id, visual.id, (v, m) => {
          const at = findUse(v);
          return at ? withoutField(v, at.role, at.index) : withField(m, v, field);
        }),
      );
      return;
    }
    // No visual selected: a new one, under the pointer if it was on the page.
    const v = newVisual(page, field.kind === "measure" ? "card" : "table", useReport.getState().pointer);
    const filled = withField(model, v, field) ?? v;
    if (await commit("Add visual", [{ op: "addVisual", page: page.id, visual: filled, index: null }])) select([v.id]);
  };

  const q = search.trim().toLowerCase();
  const tables = model.tables
    .filter((tb) => showHidden || !tb.hidden)
    .map((tb) => {
      const hit = (name: string) => !q || name.toLowerCase().includes(q) || tb.name.toLowerCase().includes(q);
      return {
        table: tb,
        measures: tb.measures.filter((m) => (showHidden || !m.hidden) && hit(m.name)),
        columns: tb.columns.filter((c) => (showHidden || !c.hidden) && hit(c.name)),
      };
    })
    .filter((x) => !q || x.measures.length || x.columns.length);

  const setAll = (expand: boolean) => setOpen(expand ? new Set(model.tables.map((x) => x.id)) : new Set());
  /** Where a menu opens: at the pointer, or under the focused item. */
  const at = (e: React.MouseEvent | React.KeyboardEvent) => {
    if (e.type === "contextmenu" && "clientX" in e) return { x: e.clientX, y: e.clientY };
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left + 24, y: r.bottom };
  };
  const menuKey = (e: React.KeyboardEvent) => e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey);
  const change = (label: string, c: Change) => commit(label, [c]);
  const paneItems = (): MenuEntry[] => [
    { id: "expand", label: t("Expand all"), icon: <ChevronsUpDown size={13} />, run: () => setAll(true) },
    { id: "collapse", label: t("Collapse all"), icon: <ChevronsDownUp size={13} />, run: () => setAll(false) },
    { id: "hidden", label: showHidden ? t("Don't show hidden fields") : t("View hidden fields"), icon: showHidden ? <EyeOff size={13} /> : <Eye size={13} />, run: () => setShowHidden((x) => !x) },
  ];
  const newItems = (tb: Table): MenuEntry[] => [
    { id: "new-measure", label: t("New measure"), icon: <Plus size={13} />, shortcut: "Ctrl+M", run: () => set({ formula: `new:${tb.id}` }) },
    { id: "new-quick", label: t("New quick measure…"), icon: <Sparkles size={13} />, run: () => set({ quick: tb.id }) },
    {
      id: "new-column",
      label: t("New column"),
      icon: <SquareFunction size={13} />,
      run: () => {
        useDataView.getState().set({ goTo: { table: tb.id, newColumn: true } });
        set({ view: "data" });
      },
    },
  ];
  const tableMenu = (tb: Table, e: React.MouseEvent | React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({
      ...at(e),
      label: tb.name,
      items: [
        ...newItems(tb),
        null,
        { id: "rename", label: t("Rename"), icon: <Pencil size={13} />, shortcut: "F2", run: () => setRenaming(`t:${tb.id}`) },
        { id: "hide", label: tb.hidden ? t("Unhide") : t("Hide"), icon: tb.hidden ? <Eye size={13} /> : <EyeOff size={13} />, run: () => change(tb.hidden ? "Unhide table" : "Hide table", { op: "setTableHidden", table: tb.id, hidden: !tb.hidden }) },
        {
          id: "date-table",
          label: tb.isDateTable ? t("Unmark as date table") : t("Mark as date table"),
          icon: <CalendarDays size={13} />,
          run: () => change(tb.isDateTable ? "Unmark as date table" : "Mark as date table", { op: "setDateTable", table: tb.id, isDateTable: !tb.isDateTable }),
        },
        null,
        ...paneItems(),
      ],
    });
  };
  const fieldMenu = (tb: Table, field: FieldRef, e: React.MouseEvent | React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    set({ field });
    const column = field.kind === "column" ? tb.columns.find((c) => c.id === field.column) : undefined;
    const measure = field.kind === "measure" ? tb.measures.find((m) => m.id === field.measure) : undefined;
    const hidden = !!(column?.hidden ?? measure?.hidden);
    const isUsed = used.has(fieldKey(field));
    // Columns from the data are removed in Transform data; calculated ones here.
    const canDelete = !!measure || column?.kind.kind === "calculated";
    const items: (MenuEntry | null)[] = [{ id: "use", label: isUsed ? t("Remove from visual") : visual ? t("Add to visual") : t("Add to a new visual"), icon: <Plus size={13} />, run: () => toggleField(field) }];
    if (measure) items.push({ id: "edit", label: t("Edit formula"), icon: <SquareFunction size={13} />, run: () => set({ formula: measure.id }) });
    items.push(null, ...newItems(tb), null);
    items.push({ id: "rename", label: t("Rename"), icon: <Pencil size={13} />, shortcut: "F2", run: () => setRenaming(fieldKey(field)) });
    items.push({
      id: "hide",
      label: hidden ? t("Unhide") : t("Hide"),
      icon: hidden ? <Eye size={13} /> : <EyeOff size={13} />,
      run: () => {
        if (column) change(hidden ? "Unhide column" : "Hide column", { op: "setColumn", table: tb.id, column: column.id, property: { name: "hidden", value: !hidden } });
        else if (measure) change(hidden ? "Unhide measure" : "Hide measure", { op: "updateMeasure", table: tb.id, measure: { ...measure, hidden: !hidden } });
      },
    });
    if (column)
      items.push({
        id: "properties",
        label: t("Properties (format, sort by)…"),
        icon: <Settings2 size={13} />,
        run: () => {
          useDataView.getState().select(tb.id, column.id);
          useDataView.getState().set({ goTo: { table: tb.id, newColumn: false }, propsOpen: true });
          set({ view: "data" });
        },
      });
    items.push({
      id: "delete",
      label: t("Delete"),
      icon: <Trash2 size={13} />,
      disabled: !canDelete,
      run: async () => {
        if (measure) {
          if (await change("Delete measure", { op: "removeMeasure", table: tb.id, measure: measure.id })) set({ formula: null, field: null });
        } else if (column) await change("Delete column", { op: "removeColumn", table: tb.id, column: column.id });
      },
    });
    items.push(null, ...paneItems());
    setMenu({ ...at(e), label: column?.name ?? measure?.name ?? "", items });
  };
  const rename = async (tb: Table, field: FieldRef | null, to: string) => {
    setRenaming(null);
    const name = to.trim();
    if (!name) return;
    if (!field) {
      if (name !== tb.name) await change("Rename table", { op: "renameTable", table: tb.id, name });
    } else if (field.kind === "column") {
      const c = tb.columns.find((x) => x.id === field.column);
      if (c && name !== c.name) await change("Rename column", { op: "renameColumn", table: tb.id, column: c.id, name });
    } else {
      const m = tb.measures.find((x) => x.id === field.measure);
      if (m && name !== m.name) await change("Rename measure", { op: "updateMeasure", table: tb.id, measure: { ...m, name } });
    }
  };
  const renameBox = (value: string, done: (to: string) => void) => (
    <input
      className="field-rename"
      defaultValue={value}
      aria-label={t("New name")}
      autoFocus
      onFocus={(e) => e.currentTarget.select()}
      onClick={(e) => e.stopPropagation()}
      onBlur={(e) => done(e.currentTarget.value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") done(e.currentTarget.value);
        else if (e.key === "Escape") setRenaming(null);
      }}
    />
  );

  const item = (tb: Table, field: FieldRef, name: string, icon: React.ReactNode, title?: string, hidden?: boolean) => {
    const isUsed = used.has(fieldKey(field));
    const isCurrent = current && sameField(current, field);
    if (renaming === fieldKey(field))
      return (
        <li key={fieldKey(field)} role="treeitem" className="field-row is-current">
          <span className="field-icon">{icon}</span>
          {renameBox(name, (to) => rename(tb, field, to))}
        </li>
      );
    return (
      <li
        key={fieldKey(field)}
        role="treeitem"
        aria-selected={!!isCurrent}
        aria-checked={isUsed}
        tabIndex={isCurrent ? 0 : -1}
        className={`field-row ${isCurrent ? "is-current" : ""} ${hidden ? "is-hidden" : ""}`}
        title={title}
        onContextMenu={(e) => fieldMenu(tb, field, e)}
        draggable
        onDragStart={(e) => {
          e.dataTransfer.setData(FIELD_MIME, JSON.stringify(field));
          e.dataTransfer.effectAllowed = "copy";
        }}
        onClick={() => {
          set({ field });
          if (field.kind === "measure") set({ formula: field.measure });
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleField(field);
          } else if (e.key === "F2") {
            e.preventDefault();
            setRenaming(fieldKey(field));
          } else if (menuKey(e)) fieldMenu(tb, field, e);
        }}
      >
        <input
          type="checkbox"
          className="field-check"
          tabIndex={-1}
          aria-label={t("Use %1", name)}
          checked={isUsed}
          onChange={() => toggleField(field)}
          onClick={(e) => e.stopPropagation()}
        />
        <span className={`field-icon ${field.kind === "measure" ? "is-measure" : ""}`}>{icon}</span>
        <span className="field-name">{name}</span>
      </li>
    );
  };

  return (
    <section className="pane fields-pane" aria-label={t("Fields")}>
      <div className="pane-head">
        <span>{t("Fields")}</span>
      </div>
      <label className="pane-search">
        <Search size={13} />
        <input value={search} placeholder={t("Search")} aria-label={t("Search fields")} onChange={(e) => setSearch(e.target.value)} />
      </label>
      {model.tables.length === 0 && (
        <div className="pane-hint fields-empty">
          <p>{t("No tables yet. Get data to see its fields here.")}</p>
          <Button size="sm" icon={<Database size={14} />} onPress={() => useReport.getState().set({ view: "query" })}>
            {t("Get data")}
          </Button>
        </div>
      )}
      <ul
        className="fields-tree"
        role="tree"
        aria-label={t("Fields")}
        onContextMenu={(e) => {
          e.preventDefault();
          const tb = model.tables.find((x) => x.id === current?.table) ?? model.tables[0];
          setMenu({ x: e.clientX, y: e.clientY, label: t("Fields"), items: [...(tb ? [...newItems(tb), null] : []), ...paneItems()] });
        }}
      >
        {tables.map(({ table: tb, measures, columns }) => {
          const isOpen = !!q || open.has(tb.id);
          return (
            <li key={tb.id} role="treeitem" aria-expanded={isOpen} onContextMenu={(e) => tableMenu(tb, e)}>
              <div className={`field-table ${tb.hidden ? "is-hidden" : ""}`}>
                {renaming === `t:${tb.id}` ? (
                  renameBox(tb.name, (to) => rename(tb, null, to))
                ) : (
                  <button
                    type="button"
                    className="field-table-btn"
                    onKeyDown={(e) => {
                      if (e.key === "F2") {
                        e.preventDefault();
                        setRenaming(`t:${tb.id}`);
                      } else if (menuKey(e)) tableMenu(tb, e);
                    }}
                    onClick={() =>
                      setOpen((s) => {
                        const n = new Set(s);
                        if (n.has(tb.id)) n.delete(tb.id);
                        else n.add(tb.id);
                        return n;
                      })
                    }
                  >
                    <ChevronRight size={13} className={`tree-caret ${isOpen ? "is-open" : ""}`} />
                    <Table2 size={14} />
                    <span>{tb.name}</span>
                  </button>
                )}
                <IconButton className="field-add" label={t("New measure in %1", tb.name)} icon={<Plus size={13} />} onPress={() => set({ formula: `new:${tb.id}` })} />
              </div>
              {isOpen && (
                <ul role="group">
                  {measures.map((m) => item(tb, { kind: "measure", table: tb.id, measure: m.id }, m.name, <MeasureIcon />, m.expression, m.hidden))}
                  {columns.map((c) => item(tb, { kind: "column", table: tb.id, column: c.id }, c.name, <ColumnIcon column={c} />, undefined, c.hidden))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </section>
  );
}
