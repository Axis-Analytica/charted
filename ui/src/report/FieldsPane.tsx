// The Fields pane: tables with their measures, columns and hierarchies, grouped into
// display folders (nested with `\`, as Power BI does). Fields can be dragged to the
// canvas, a visual, a field well or the filters pane (a hierarchy brings its levels), or
// onto a folder to move them into it; ticking one adds it to the selected visual (or
// creates one), and Enter does the same from the keyboard. Hovering shows a field's
// description. Right-click (or Shift+F10) on a table, a field or the space around them
// opens a menu to add measures, columns, hierarchies and groups, rename (F2), hide, edit
// properties and delete. "New table" opens the formula bar for a calculated table, and
// "New parameter…" makes a what-if parameter.
import {
  CalendarDays,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Database,
  Eye,
  EyeOff,
  Folder,
  FolderOpen,
  Group,
  ListTree,
  Network,
  Pencil,
  Plus,
  Search,
  Settings2,
  Sparkles,
  SlidersHorizontal,
  SquareFunction,
  TableProperties,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { useMemo, useState } from "react";
import type { Change } from "../bridge/generated/Change";
import type { Column } from "../bridge/generated/Column";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Hierarchy } from "../bridge/generated/Hierarchy";
import type { Measure } from "../bridge/generated/Measure";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Table } from "../bridge/generated/Table";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { ContextMenu, type ContextMenuState } from "../kit/ContextMenu";
import type { MenuEntry } from "../kit/Menu";
import { useApp } from "../state/app";
import { useDataView } from "../state/dataview";
import { editVisual, fieldKey, newId, newVisual, sameField, useReport, withField, withFields, withoutField } from "../state/report";
import { ColumnIcon, MeasureIcon, TableIcon } from "../views/icons";
import { FIELD_MIME } from "./Canvas";
import { FieldProperties, type PropsTarget } from "./FieldProperties";
import { type FolderNode, folderString, folderTree, hierarchyFields, newHierarchy, withLevel } from "./fieldTree";
import { GroupDialog, type GroupTarget } from "./GroupDialog";
import { ParameterDialog } from "./ParameterDialog";
import { groupKind, groupOf } from "./groups";

/** Which model object a Fields pane drag carries, for dropping it onto a folder. */
const ITEM_MIME = "application/x-charted-model-item";

type Item =
  | { kind: "measure"; id: string; name: string; displayFolder?: string | null; hidden: boolean; measure: Measure }
  | { kind: "column"; id: string; name: string; displayFolder?: string | null; hidden: boolean; column: Column }
  | { kind: "hierarchy"; id: string; name: string; displayFolder?: string | null; hidden: boolean; hierarchy: Hierarchy };

/** The change that moves an item into a display folder (null: the table's root). */
function moveTo(table: string, item: Item, folder: string | null): Change | null {
  if ((item.displayFolder ?? null) === folder) return null;
  switch (item.kind) {
    case "measure":
      return { op: "updateMeasure", table, measure: { ...item.measure, displayFolder: folder } };
    case "column":
      return { op: "setColumn", table, column: item.id, property: { name: "displayFolder", value: folder } };
    case "hierarchy":
      return { op: "updateHierarchy", table, hierarchy: { ...item.hierarchy, displayFolder: folder } };
  }
}

function itemsOf(tb: Table): Item[] {
  return [
    ...tb.measures.map((m): Item => ({ kind: "measure", id: m.id, name: m.name, displayFolder: m.displayFolder, hidden: m.hidden, measure: m })),
    ...tb.columns.map((c): Item => ({ kind: "column", id: c.id, name: c.name, displayFolder: c.displayFolder, hidden: c.hidden, column: c })),
    ...(tb.hierarchies ?? []).map((h): Item => ({ kind: "hierarchy", id: h.id, name: h.name, displayFolder: h.displayFolder, hidden: h.hidden, hierarchy: h })),
  ];
}

export function FieldsPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const edit = useApp((s) => s.edit);
  const { selected, select, field: current, set } = useReport();
  const calcErrors = useApp((s) => s.state?.project?.calcErrors);
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState<Set<string>>(() => new Set(model.tables.slice(0, 1).map((x) => x.id)));
  /** Folders are open until closed; hierarchies closed until opened. */
  const [closedFolders, setClosedFolders] = useState<Set<string>>(() => new Set());
  const [openHierarchies, setOpenHierarchies] = useState<Set<string>>(() => new Set());
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  /** What is being renamed: `t:<table id>`, `h:<hierarchy id>` or a field's key. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [props, setProps] = useState<PropsTarget | null>(null);
  const [group, setGroup] = useState<GroupTarget | null>(null);
  const [parameter, setParameter] = useState(false);
  /** The folder a drag is over. */
  const [dropAt, setDropAt] = useState<string | null>(null);
  const visual = selected.length === 1 ? page.visuals.find((v) => v.id === selected[0]) : undefined;

  /** Where a field is used in the selected visual. */
  const used = useMemo(() => {
    const m = new Map<string, { role: string; index: number }>();
    if (visual) for (const [role, ps] of Object.entries(visual.roles)) (ps ?? []).forEach((p, index) => m.set(fieldKey(p.field), { role, index }));
    return m;
  }, [visual]);

  /** Ticks or unticks fields: all of them are removed when all are used, else the missing ones are added. */
  const toggleFields = async (fields: FieldRef[]) => {
    if (!fields.length) return;
    if (visual) {
      // Decide from the latest state: an earlier tick may still be landing.
      const uses = (v: Visual) => Object.entries(v.roles).flatMap(([role, ps]) => (ps ?? []).map((p, index) => ({ role, index, p })));
      await edit(
        "Change fields",
        editVisual(page.id, visual.id, (v, m) => {
          const all = uses(v).filter((x) => fields.some((f) => sameField(f, x.p.field)));
          if (all.length >= fields.length) return all.reverse().reduce((acc, x) => withoutField(acc, x.role, x.index), v);
          const missing = fields.filter((f) => !all.some((x) => sameField(f, x.p.field)));
          return missing.length === 1 ? withField(m, v, missing[0]!) : withFields(m, v, missing);
        }),
      );
      return;
    }
    // No visual selected: a new one, under the pointer if it was on the page.
    const v = newVisual(page, fields.length === 1 && fields[0]!.kind === "measure" ? "card" : "table", useReport.getState().pointer);
    const filled = withFields(model, v, fields) ?? v;
    if (await commit("Add visual", [{ op: "addVisual", page: page.id, visual: filled, index: null }])) select([v.id]);
  };
  const toggleField = (field: FieldRef) => toggleFields([field]);

  const q = search.trim().toLowerCase();
  const tables = model.tables
    .filter((tb) => showHidden || !tb.hidden)
    .map((tb) => {
      const hit = (it: Item) =>
        !q ||
        [it.name, tb.name, it.displayFolder ?? "", ...(it.kind === "hierarchy" ? it.hierarchy.levels.map((l) => l.name) : [])].some((s) => s.toLowerCase().includes(q));
      const items = itemsOf(tb).filter((it) => (showHidden || !it.hidden) && hit(it));
      return { table: tb, items, tree: folderTree(items, (it) => it.displayFolder) };
    })
    .filter((x) => !q || x.items.length);

  const setAll = (expand: boolean) => {
    setOpen(expand ? new Set(model.tables.map((x) => x.id)) : new Set());
    if (expand) setClosedFolders(new Set());
  };
  /** Where a menu opens: at the pointer, or under the focused item. */
  const at = (e: React.MouseEvent | React.KeyboardEvent) => {
    if (e.type === "contextmenu" && "clientX" in e) return { x: e.clientX, y: e.clientY };
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left + 24, y: r.bottom };
  };
  const menuKey = (e: React.KeyboardEvent) => e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey);
  const change = (label: string, c: Change) => commit(label, [c]);
  const paneItems = (): MenuEntry[] => [
    { id: "new-table", label: t("New table"), icon: <TableProperties size={13} />, run: () => set({ formula: "table:new" }) },
    { id: "new-parameter", label: t("New parameter…"), icon: <SlidersHorizontal size={13} />, run: () => setParameter(true) },
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
        ...(tb.calculated
          ? [
              { id: "edit-table", label: t("Edit formula"), icon: <SquareFunction size={13} />, run: () => set({ formula: `table:${tb.id}` }) },
              {
                id: "delete-table",
                label: t("Delete table"),
                icon: <Trash2 size={13} />,
                run: async () => {
                  if (await change("Delete table", { op: "removeTable", table: tb.id })) set({ formula: null, field: null });
                },
              },
            ]
          : []),
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
  const createHierarchy = async (tb: Table, column: Column) => {
    const h = newHierarchy(tb, column, newId());
    if (await change("Create hierarchy", { op: "addHierarchy", table: tb.id, hierarchy: h, index: null })) {
      setOpenHierarchies((s) => new Set(s).add(h.id));
      setRenaming(`h:${h.id}`);
    }
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
    items.push(null, ...newItems(tb));
    if (column) {
      items.push({ id: "new-hierarchy", label: t("Create hierarchy"), icon: <Network size={13} />, run: () => createHierarchy(tb, column) });
      for (const h of tb.hierarchies ?? []) {
        if (h.levels.some((l) => l.column === column.id)) continue;
        items.push({
          id: `add-to-${h.id}`,
          label: t("Add to hierarchy %1", h.name),
          icon: <ListTree size={13} />,
          run: () =>
            edit("Add to hierarchy", (m) => {
              const now = m.tables.find((x) => x.id === tb.id)?.hierarchies?.find((x) => x.id === h.id);
              return now ? [{ op: "updateHierarchy", table: tb.id, hierarchy: withLevel(now, column) }] : null;
            }),
        });
      }
      const g = groupOf(column);
      if (g) items.push({ id: "edit-group", label: t("Edit groups…"), icon: <Group size={13} />, run: () => setGroup({ table: tb.id, column: column.id }) });
      else if (groupKind(column.dataType) === "bin" || column.dataType === "String")
        items.push({ id: "new-group", label: t("New group…"), icon: <Group size={13} />, run: () => setGroup({ table: tb.id, column: column.id }) });
    }
    items.push(null);
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
    items.push({
      id: "properties",
      label: t("Properties…"),
      icon: <Settings2 size={13} />,
      run: () => setProps({ kind: column ? "column" : "measure", table: tb.id, id: column?.id ?? measure!.id }),
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
  const hierarchyMenu = (tb: Table, h: Hierarchy, e: React.MouseEvent | React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const fields = hierarchyFields(tb, h);
    const allUsed = fields.length > 0 && fields.every((f) => used.has(fieldKey(f)));
    setMenu({
      ...at(e),
      label: h.name,
      items: [
        { id: "use", label: allUsed ? t("Remove from visual") : visual ? t("Add to visual") : t("Add to a new visual"), icon: <Plus size={13} />, run: () => toggleFields(fields) },
        null,
        { id: "rename", label: t("Rename"), icon: <Pencil size={13} />, shortcut: "F2", run: () => setRenaming(`h:${h.id}`) },
        {
          id: "hide",
          label: h.hidden ? t("Unhide") : t("Hide"),
          icon: h.hidden ? <Eye size={13} /> : <EyeOff size={13} />,
          run: () => change(h.hidden ? "Unhide hierarchy" : "Hide hierarchy", { op: "updateHierarchy", table: tb.id, hierarchy: { ...h, hidden: !h.hidden } }),
        },
        { id: "properties", label: t("Properties…"), icon: <Settings2 size={13} />, run: () => setProps({ kind: "hierarchy", table: tb.id, id: h.id }) },
        { id: "delete", label: t("Delete"), icon: <Trash2 size={13} />, run: () => change("Delete hierarchy", { op: "removeHierarchy", table: tb.id, hierarchy: h.id }) },
        null,
        ...paneItems(),
      ],
    });
  };
  const rename = async (tb: Table, field: FieldRef | Hierarchy | null, to: string) => {
    setRenaming(null);
    const name = to.trim();
    if (!name) return;
    if (!field) {
      if (name !== tb.name) await change("Rename table", { op: "renameTable", table: tb.id, name });
    } else if ("levels" in field) {
      if (name !== field.name) await change("Rename hierarchy", { op: "updateHierarchy", table: tb.id, hierarchy: { ...field, name } });
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

  /** Drop targets that move dragged fields of the table into a folder (`null`: its root). */
  const folderDrop = (tb: Table, folder: string | null) => ({
    onDragOver: (e: React.DragEvent) => {
      if (!e.dataTransfer.types.includes(ITEM_MIME)) return;
      e.preventDefault();
      e.stopPropagation();
      e.dataTransfer.dropEffect = "move";
      setDropAt(`${tb.id}\n${folder ?? ""}`);
    },
    onDragLeave: () => setDropAt(null),
    onDrop: (e: React.DragEvent) => {
      const raw = e.dataTransfer.getData(ITEM_MIME);
      setDropAt(null);
      if (!raw) return;
      e.preventDefault();
      e.stopPropagation();
      const drag = JSON.parse(raw) as { table: string; kind: Item["kind"]; id: string };
      if (drag.table !== tb.id) return;
      edit("Move to folder", (m) => {
        const now = m.tables.find((x) => x.id === tb.id);
        const it = now && itemsOf(now).find((x) => x.kind === drag.kind && x.id === drag.id);
        const c = it ? moveTo(tb.id, it, folder) : null;
        return c ? [c] : null;
      });
    },
  });
  const dragStart = (tb: Table, it: Item, fields: FieldRef | FieldRef[]) => (e: React.DragEvent) => {
    e.dataTransfer.setData(FIELD_MIME, JSON.stringify(fields));
    e.dataTransfer.setData(ITEM_MIME, JSON.stringify({ table: tb.id, kind: it.kind, id: it.id }));
    e.dataTransfer.effectAllowed = "copyMove";
  };
  /** Field rows start at their checkbox; folders and hierarchies at their caret, left of it. */
  const indent = (depth: number, caret = false) => ({ paddingInlineStart: (caret ? 8 : 22) + depth * 14 });

  const fieldRow = (tb: Table, field: FieldRef, name: string, icon: React.ReactNode, depth: number, opts: { title?: string; hidden?: boolean; item?: Item; level?: boolean } = {}) => {
    const isUsed = used.has(fieldKey(field));
    const isCurrent = current && sameField(current, field);
    const key = `${opts.level ? "l:" : ""}${fieldKey(field)}`;
    if (renaming === fieldKey(field) && !opts.level)
      return (
        <li key={key} role="treeitem" className="field-row is-current" style={indent(depth)}>
          <span className="field-icon">{icon}</span>
          {renameBox(name, (to) => rename(tb, field, to))}
        </li>
      );
    return (
      <li
        key={key}
        role="treeitem"
        aria-selected={!!isCurrent}
        aria-checked={isUsed}
        tabIndex={isCurrent ? 0 : -1}
        className={`field-row ${isCurrent ? "is-current" : ""} ${opts.hidden ? "is-hidden" : ""} ${opts.level ? "is-level" : ""}`}
        style={indent(depth)}
        title={opts.title}
        onContextMenu={(e) => fieldMenu(tb, field, e)}
        draggable
        onDragStart={
          opts.item
            ? dragStart(tb, opts.item, field)
            : (e) => {
                e.dataTransfer.setData(FIELD_MIME, JSON.stringify(field));
                e.dataTransfer.effectAllowed = "copy";
              }
        }
        onClick={() => {
          set({ field });
          if (field.kind === "measure") set({ formula: field.measure });
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleField(field);
          } else if (e.key === "F2" && !opts.level) {
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

  const hierarchyRow = (tb: Table, it: Extract<Item, { kind: "hierarchy" }>, depth: number) => {
    const h = it.hierarchy;
    const fields = hierarchyFields(tb, h);
    const isUsed = fields.length > 0 && fields.every((f) => used.has(fieldKey(f)));
    const isOpen = !!q || openHierarchies.has(h.id);
    const toggleOpen = () =>
      setOpenHierarchies((s) => {
        const n = new Set(s);
        if (n.has(h.id)) n.delete(h.id);
        else n.add(h.id);
        return n;
      });
    const title = h.description ?? h.levels.map((l) => l.name).join(" › ");
    return (
      <li key={`h:${h.id}`} role="treeitem" aria-expanded={isOpen} className="field-hierarchy" onContextMenu={(e) => hierarchyMenu(tb, h, e)}>
        {renaming === `h:${h.id}` ? (
          <div key="rename" className="field-row is-current" style={indent(depth)}>
            <span className="field-icon">
              <Network size={14} />
            </span>
            {renameBox(h.name, (to) => rename(tb, h, to))}
          </div>
        ) : (
          <div
            key="row"
            className={`field-row is-hierarchy ${h.hidden ? "is-hidden" : ""}`}
            style={indent(depth, true)}
            title={title}
            tabIndex={-1}
            draggable
            onDragStart={dragStart(tb, it, fields)}
            onClick={toggleOpen}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                toggleFields(fields);
              } else if (e.key === "F2") {
                e.preventDefault();
                setRenaming(`h:${h.id}`);
              } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") toggleOpen();
              else if (menuKey(e)) hierarchyMenu(tb, h, e);
            }}
          >
            <ChevronRight size={12} className={`tree-caret field-caret ${isOpen ? "is-open" : ""}`} />
            <input
              type="checkbox"
              className="field-check"
              tabIndex={-1}
              aria-label={t("Use %1", h.name)}
              checked={isUsed}
              onChange={() => toggleFields(fields)}
              onClick={(e) => e.stopPropagation()}
            />
            <span className="field-icon">
              <Network size={14} />
            </span>
            <span className="field-name">{h.name}</span>
          </div>
        )}
        {isOpen && (
          <ul role="group" aria-label={h.name}>
            {h.levels.map((l) => {
              const c = tb.columns.find((x) => x.id === l.column);
              return c ? fieldRow(tb, { kind: "column", table: tb.id, column: c.id }, l.name, <ColumnIcon column={c} />, depth + 1, { title: c.description ?? c.name, level: true }) : null;
            })}
          </ul>
        )}
      </li>
    );
  };

  const itemRow = (tb: Table, it: Item, depth: number) => {
    switch (it.kind) {
      case "measure":
        return fieldRow(tb, { kind: "measure", table: tb.id, measure: it.id }, it.name, <MeasureIcon />, depth, { title: it.measure.description ?? it.measure.expression, hidden: it.hidden, item: it });
      case "column":
        return fieldRow(tb, { kind: "column", table: tb.id, column: it.id }, it.name, <ColumnIcon column={it.column} />, depth, { title: it.column.description ?? undefined, hidden: it.hidden, item: it });
      case "hierarchy":
        return hierarchyRow(tb, it, depth);
    }
  };

  const folderRows = (tb: Table, node: FolderNode<Item>, depth: number): React.ReactNode[] => [
    ...node.folders.map((f) => {
      const key = `${tb.id}\n${f.path}`;
      const isOpen = !!q || !closedFolders.has(key);
      return (
        <li key={`f:${f.path}`} role="treeitem" aria-expanded={isOpen} aria-label={f.name} className="field-folder-item">
          <button
            type="button"
            className={`field-folder ${dropAt === key ? "is-over" : ""}`}
            style={indent(depth, true)}
            title={f.path}
            {...folderDrop(tb, folderString(f.path.split("\\")))}
            onClick={() =>
              setClosedFolders((s) => {
                const n = new Set(s);
                if (n.has(key)) n.delete(key);
                else n.add(key);
                return n;
              })
            }
          >
            <ChevronRight size={12} className={`tree-caret ${isOpen ? "is-open" : ""}`} />
            {isOpen ? <FolderOpen size={14} /> : <Folder size={14} />}
            <span>{f.name}</span>
          </button>
          {isOpen && <ul role="group">{folderRows(tb, f, depth + 1)}</ul>}
        </li>
      );
    }),
    ...node.items.map((it) => itemRow(tb, it, depth)),
  ];

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
        {tables.map(({ table: tb, tree }) => {
          const isOpen = !!q || open.has(tb.id);
          const rootKey = `${tb.id}\n`;
          return (
            <li key={tb.id} role="treeitem" aria-expanded={isOpen} onContextMenu={(e) => tableMenu(tb, e)}>
              <div className={`field-table ${tb.hidden ? "is-hidden" : ""} ${dropAt === rootKey ? "is-over" : ""}`} {...folderDrop(tb, null)}>
                {renaming === `t:${tb.id}` ? (
                  renameBox(tb.name, (to) => rename(tb, null, to))
                ) : (
                  <button
                    type="button"
                    className="field-table-btn"
                    title={tb.description ?? undefined}
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
                    <TableIcon table={tb} />
                    <span>{tb.name}</span>
                    {calcErrors?.[tb.id] && (
                      <span className="field-error" role="img" aria-label={t("Error: %1", calcErrors[tb.id]!)} title={calcErrors[tb.id]}>
                        <TriangleAlert size={13} />
                      </span>
                    )}
                  </button>
                )}
                <IconButton className="field-add" label={t("New measure in %1", tb.name)} icon={<Plus size={13} />} onPress={() => set({ formula: `new:${tb.id}` })} />
              </div>
              {isOpen && <ul role="group">{folderRows(tb, tree, 0)}</ul>}
            </li>
          );
        })}
      </ul>
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
      {props && <FieldProperties model={model} target={props} onClose={() => setProps(null)} />}
      {group && <GroupDialog model={model} target={group} onClose={() => setGroup(null)} />}
      {parameter && <ParameterDialog model={model} page={page} onClose={() => setParameter(false)} />}
    </section>
  );
}
