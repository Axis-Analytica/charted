// The Fields pane: tables with their measures and columns. Fields can be dragged to the
// canvas, a visual, a field well or the filters pane; ticking one adds it to the
// selected visual (or creates one), and Enter does the same from the keyboard.
import { ChevronRight, Plus, Search, Table2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import { useT } from "../i18n";
import { IconButton } from "../kit/Button";
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
    .filter((tb) => !tb.hidden)
    .map((tb) => {
      const hit = (name: string) => !q || name.toLowerCase().includes(q) || tb.name.toLowerCase().includes(q);
      return {
        table: tb,
        measures: tb.measures.filter((m) => !m.hidden && hit(m.name)),
        columns: tb.columns.filter((c) => !c.hidden && hit(c.name)),
      };
    })
    .filter((x) => !q || x.measures.length || x.columns.length);

  const item = (field: FieldRef, name: string, icon: React.ReactNode, title?: string) => {
    const isUsed = used.has(fieldKey(field));
    const isCurrent = current && sameField(current, field);
    return (
      <li
        key={fieldKey(field)}
        role="treeitem"
        aria-selected={!!isCurrent}
        aria-checked={isUsed}
        tabIndex={isCurrent ? 0 : -1}
        className={`field-row ${isCurrent ? "is-current" : ""}`}
        title={title}
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
          }
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
      <ul className="fields-tree" role="tree" aria-label={t("Fields")}>
        {tables.map(({ table: tb, measures, columns }) => {
          const isOpen = !!q || open.has(tb.id);
          return (
            <li key={tb.id} role="treeitem" aria-expanded={isOpen}>
              <div className="field-table">
                <button
                  type="button"
                  className="field-table-btn"
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
                <IconButton className="field-add" label={t("New measure in %1", tb.name)} icon={<Plus size={13} />} onPress={() => set({ formula: `new:${tb.id}` })} />
              </div>
              {isOpen && (
                <ul role="group">
                  {measures.map((m) => item({ kind: "measure", table: tb.id, measure: m.id }, m.name, <MeasureIcon />, m.expression))}
                  {columns.map((c) => item({ kind: "column", table: tb.id, column: c.id }, c.name, <ColumnIcon column={c} />))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
