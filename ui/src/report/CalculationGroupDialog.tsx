// The calculation group dialog: a group's name, precedence and items. Each item is a
// DAX expression in which SELECTEDMEASURE () stands for the measure it's applied to
// (CALCULATE ( SELECTEDMEASURE (), DATESYTD ( 'Date'[Date] ) )). Put the group's Name
// column on a visual's axis, or pick an item in a slicer, and the visual's measures
// show with the items applied (the core rewrites them, see calcgroups.rs). Saving is
// one undoable commit; the core's message (an unknown function, say) shows in place.
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { CalculationItem } from "../bridge/generated/CalculationItem";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Table } from "../bridge/generated/Table";
import { DaxEditor } from "../dax/LazyDaxEditor";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { newId } from "../state/report";
import { calculationGroupTable, cleanItems, itemsProblem, SELECTED_MEASURE } from "./calcgroups";
import { uniqueTableName } from "./FormulaBar";

/** `table` is the group to edit; without it, a new one. `onSaved` gets the table's id. */
export function CalculationGroupDialog({ model, table, onClose, onSaved }: { model: Model; table?: Table; onClose(): void; onSaved?(id: string): void }) {
  const { t } = useT();
  const group = table?.calculationGroup;
  const [name, setName] = useState(() => table?.name ?? uniqueTableName(model, t("Calculation group")));
  const [precedence, setPrecedence] = useState(String(group?.precedence ?? 0));
  const [items, setItems] = useState<CalculationItem[]>(() => group?.items ?? [{ name: t("Calculation item"), expression: SELECTED_MEASURE, formatString: null }]);
  const [at, setAt] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const current = items[at];
  const edit = (patch: Partial<CalculationItem>) => {
    setItems((xs) => xs.map((x, i) => (i === at ? { ...x, ...patch } : x)));
    setError(null);
  };
  const move = (by: number) => {
    const to = at + by;
    if (to < 0 || to >= items.length) return;
    setItems((xs) => {
      const out = [...xs];
      [out[at], out[to]] = [out[to]!, out[at]!];
      return out;
    });
    setAt(to);
  };
  const add = () => {
    const taken = new Set(items.map((x) => x.name.toLowerCase()));
    let n = items.length + 1;
    while (taken.has(`${t("Calculation item")} ${n}`.toLowerCase())) n++;
    setItems((xs) => [...xs, { name: `${t("Calculation item")} ${n}`, expression: SELECTED_MEASURE, formatString: null }]);
    setAt(items.length);
  };
  const remove = () => {
    setItems((xs) => xs.filter((_, i) => i !== at));
    setAt((i) => Math.max(0, i - 1));
  };

  const trimmed = name.trim();
  const taken = model.tables.some((tb) => tb.id !== table?.id && tb.name.toLowerCase() === trimmed.toLowerCase());
  const level = Number(precedence);
  const problem = !trimmed
    ? t("Enter a name.")
    : taken
      ? t("A table named %1 already exists.", trimmed)
      : precedence.trim() === "" || !Number.isInteger(level)
        ? t("Precedence is a whole number.")
        : itemsProblem(items) && t(itemsProblem(items)!);

  const save = async () => {
    if (problem || busy) return;
    const g = { precedence: level, items: cleanItems(items) };
    let changes: Change[];
    let id: string;
    if (table) {
      id = table.id;
      changes = [];
      if (trimmed !== table.name) changes.push({ op: "renameTable", table: id, name: trimmed });
      changes.push({ op: "setCalculationGroup", table: id, group: g });
    } else {
      const tb = calculationGroupTable(trimmed, g, newId);
      id = tb.id;
      changes = [{ op: "addTable", table: tb, index: null }];
    }
    setBusy(true);
    try {
      const state = await transport.rpc({ type: "commit", label: table ? "Edit calculation group" : "New calculation group", changes });
      useApp.setState({ state });
      onSaved?.(id);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      modal
      onOpenChange={(o) => !o && onClose()}
      title={table ? t("Edit calculation group") : t("New calculation group")}
      width={720}
      footer={
        <>
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy || !!problem} onPress={save}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="calc-group">
        <div className="calc-group-head">
          <label className="field">
            <span className="field-label">{t("Name")}</span>
            <input className="input" aria-label={t("Name")} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="field">
            <span className="field-label">{t("Precedence")}</span>
            <input className="input" type="number" step="1" aria-label={t("Precedence")} value={precedence} onChange={(e) => setPrecedence(e.target.value)} />
          </label>
        </div>
        <div className="calc-group-body">
          <div className="calc-group-items">
            <span className="field-label">{t("Items")}</span>
            <ul className="group-values" aria-label={t("Items")}>
              {items.map((it, i) => (
                <li key={i}>
                  <button type="button" className="calc-group-pick" aria-pressed={i === at} onClick={() => setAt(i)}>
                    {it.name || t("(no name)")}
                  </button>
                </li>
              ))}
            </ul>
            <div className="calc-group-tools">
              <IconButton label={t("Add item")} icon={<Plus size={14} />} onPress={add} />
              <IconButton label={t("Move up")} icon={<ArrowUp size={14} />} isDisabled={at === 0} onPress={() => move(-1)} />
              <IconButton label={t("Move down")} icon={<ArrowDown size={14} />} isDisabled={at >= items.length - 1} onPress={() => move(1)} />
              <IconButton label={t("Delete item")} icon={<Trash2 size={14} />} isDisabled={items.length <= 1} onPress={remove} />
            </div>
          </div>
          {current && (
            <div className="calc-group-item">
              <label className="field">
                <span className="field-label">{t("Item name")}</span>
                <input className="input" aria-label={t("Item name")} value={current.name} onChange={(e) => edit({ name: e.target.value })} />
              </label>
              <div className="field">
                <span className="field-label">{t("Expression")}</span>
                <DaxEditor key={at} label={t("Item expression")} model={model} item value={current.expression} onChange={(expression) => edit({ expression })} onCommit={save} onCancel={onClose} maxLines={10} />
              </div>
              <label className="field">
                <span className="field-label">{t("Format string (optional)")}</span>
                <input className="input mono" aria-label={t("Format string")} placeholder={'"0.0%"'} value={current.formatString ?? ""} onChange={(e) => edit({ formatString: e.target.value })} />
              </label>
              <p className="field-hint">{t("SELECTEDMEASURE () is the measure the item is applied to. Put the group's Name column on an axis or in a slicer to apply its items.")}</p>
            </div>
          )}
        </div>
        {(problem || error) && (
          <p className="field-error" role="alert">
            {problem ?? error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
