import { ChevronRight, Table2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import type { Table } from "../bridge/generated/Table";
import { formatNumber, useT } from "../i18n";
import { useApp } from "../state/app";
import { useShortcut } from "../app/shortcuts";
import { useDataView } from "../state/dataview";
import { ColumnIcon, MeasureIcon } from "./icons";

function RenameInput({ initial, onDone }: { initial: string; onDone(name: string | null): void }) {
  const { t } = useT();
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  return (
    <input
      ref={ref}
      className="rename-input"
      aria-label={t("New name")}
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => onDone(value.trim() && value.trim() !== initial ? value.trim() : null)}
      onKeyDown={(e) => {
        if (e.key === "Enter") onDone(value.trim() && value.trim() !== initial ? value.trim() : null);
        if (e.key === "Escape") onDone(null);
        e.stopPropagation();
      }}
    />
  );
}

export function TablesPanel({
  project,
  selected,
  onSelect,
  onSelectColumn,
}: {
  project: ProjectSnapshot;
  selected: string | null;
  onSelect(id: string): void;
  onSelectColumn(table: string, column: string): void;
}) {
  const selectedColumns = useDataView((s) => s.selected);
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const toggle = (id: string) =>
    setExpanded((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const rename = async (table: Table, name: string | null) => {
    setRenaming(null);
    if (name) await commit("Rename table", [{ op: "renameTable", table: table.id, name }]);
  };
  useShortcut({ id: "rename", keys: "F2", label: "Rename table", group: "Edit", run: () => selected && setRenaming(selected), enabled: () => !!selected });

  const move = (delta: number) => {
    const i = project.model.tables.findIndex((x) => x.id === selected);
    const next = project.model.tables[Math.min(project.model.tables.length - 1, Math.max(0, i + delta))];
    if (next) onSelect(next.id);
  };

  return (
    <aside className="sidebar" aria-label={t("Tables")}>
      <div className="sidebar-head">
        <span>{t("Tables")}</span>
        <span className="sidebar-count">{project.model.tables.length}</span>
      </div>
      <ul
        className="tree"
        role="tree"
        aria-label={t("Tables")}
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.altKey || e.ctrlKey || e.metaKey) return;
          if (e.key === "ArrowDown") (e.preventDefault(), move(1));
          if (e.key === "ArrowUp") (e.preventDefault(), move(-1));
          if (e.key === "ArrowRight" && selected) setExpanded((s) => new Set(s).add(selected));
          if (e.key === "ArrowLeft" && selected)
            setExpanded((s) => {
              const n = new Set(s);
              n.delete(selected);
              return n;
            });
        }}
      >
        {project.model.tables.map((table) => {
          const rows = project.tables.find((s) => s.id === table.id)?.rows ?? 0;
          const open = expanded.has(table.id);
          return (
            <li key={table.id} role="treeitem" aria-expanded={open} aria-selected={selected === table.id}>
              <div className={`tree-row ${selected === table.id ? "is-selected" : ""}`} onClick={() => onSelect(table.id)} onDoubleClick={() => setRenaming(table.id)}>
                <button
                  type="button"
                  className={`tree-chevron ${open ? "is-open" : ""}`}
                  aria-label={open ? t("Collapse") : t("Expand")}
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation();
                    toggle(table.id);
                  }}
                >
                  <ChevronRight size={13} />
                </button>
                <Table2 size={14} className="tree-icon" />
                {renaming === table.id ? (
                  <RenameInput initial={table.name} onDone={(n) => rename(table, n)} />
                ) : (
                  <span className="tree-label">{table.name}</span>
                )}
                {table.isDateTable && <span className="tree-tag">{t("Date")}</span>}
                <span className="tree-meta tabular">{formatNumber(Number(rows), { notation: "compact" })}</span>
              </div>
              {open && (
                <ul role="group" className="tree-children">
                  {table.measures.map((m) => (
                    <li key={m.id} role="treeitem" className="tree-row tree-leaf" title={m.expression}>
                      <span className="tree-icon is-measure"><MeasureIcon /></span>
                      <span className="tree-label">{m.name}</span>
                    </li>
                  ))}
                  {table.columns.map((c) => (
                    <li
                      key={c.id}
                      role="treeitem"
                      aria-selected={selected === table.id && selectedColumns[table.id] === c.id}
                      className={`tree-row tree-leaf ${c.hidden ? "is-hidden" : ""} ${selected === table.id && selectedColumns[table.id] === c.id ? "is-current" : ""}`}
                      onClick={() => onSelectColumn(table.id, c.id)}
                    >
                      <span className="tree-icon"><ColumnIcon column={c} /></span>
                      <span className="tree-label">{c.name}</span>
                      {c.isKey && <span className="tree-tag">{t("Key")}</span>}
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </aside>
  );
}
