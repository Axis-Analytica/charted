// The Data view's formula bar: write a calculated column as `Name = expression`,
// evaluated for each row of the table (its columns and RELATED lookups are in reach).
// charted computes the values and works out their type when you commit.
import { Check, Trash2, WandSparkles, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { errorMessage, transport } from "../../bridge";
import type { Change } from "../../bridge/generated/Change";
import type { Column } from "../../bridge/generated/Column";
import type { Model } from "../../bridge/generated/Model";
import type { Table } from "../../bridge/generated/Table";
import { DaxEditor, type DaxEditorHandle } from "../../dax/DaxEditor";
import { splitFormula } from "../../dax/service";
import { useT } from "../../i18n";
import { IconButton } from "../../kit/Button";
import { useApp } from "../../state/app";
import { newId } from "../../state/report";

function uniqueColumnName(table: Table, base: string): string {
  const taken = new Set(table.columns.map((c) => c.name.toLowerCase()));
  if (!taken.has(base.toLowerCase())) return base;
  for (let i = 2; ; i++) if (!taken.has(`${base} ${i}`.toLowerCase())) return `${base} ${i}`;
}

/** `column` is a calculated column to edit, or null for a new one. */
export function ColumnFormulaBar({ model, table, column, onDone, onCreated }: { model: Model; table: Table; column: Column | null; onDone(): void; onCreated(id: string): void }) {
  const { t } = useT();
  const fallback = uniqueColumnName(table, t("Column"));
  const expression = column?.kind.kind === "calculated" ? column.kind.expression : "";
  const initial = column ? `${column.name} = ${expression}` : `${fallback} = `;
  const [text, setText] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<DaxEditorHandle>(null);

  useEffect(() => {
    setText(initial);
    setError(null);
    requestAnimationFrame(() => ref.current?.focus());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [column?.id, expression, column?.name]);

  const { name, exprStart } = splitFormula(text);
  const expr = text.slice(exprStart).trim();
  const dirty = text !== initial;

  const save = async () => {
    if (!expr || busy) return;
    if (column && !dirty) return;
    const changes: Change[] = [];
    let created: string | null = null;
    if (column) {
      if (name && name !== column.name) changes.push({ op: "renameColumn", table: table.id, column: column.id, name });
      if (expr !== expression) changes.push({ op: "setColumn", table: table.id, column: column.id, property: { name: "expression", value: expr } });
    } else {
      created = newId();
      const c: Column = {
        id: created,
        name: name ?? fallback,
        dataType: "Unknown",
        kind: { kind: "calculated", expression: expr, language: "Dax" },
        description: null,
        formatString: null,
        summarizeBy: "Default",
        dataCategory: null,
        sortBy: null,
        displayFolder: null,
        hidden: false,
        isKey: false,
      };
      changes.push({ op: "addColumn", table: table.id, column: c, index: null });
    }
    if (!changes.length) return;
    setBusy(true);
    setError(null);
    try {
      const state = await transport.rpc({ type: "commit", label: column ? "Edit column" : "New column", changes });
      useApp.setState({ state });
      if (created) onCreated(created);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const remove = async () => {
    if (column && (await useApp.getState().commit("Delete column", [{ op: "removeColumn", table: table.id, column: column.id }]))) onDone();
  };

  return (
    <div className={`formula-bar ${error ? "has-error" : ""}`} role="group" aria-label={t("Column formula bar")}>
      <IconButton label={t("Cancel")} shortcut="Escape" icon={<X size={15} />} onPress={() => (column ? setText(initial) : onDone())} />
      <IconButton label={t("Commit")} shortcut="Enter" icon={<Check size={15} />} isDisabled={busy || !expr || (!!column && !dirty)} onPress={save} />
      <div className="formula-edit">
        <DaxEditor ref={ref} label={t("Column formula")} model={model} home={table.id} value={text} onChange={setText} onCommit={save} onCancel={() => (column && dirty ? setText(initial) : onDone())} />
        {error && (
          <div className="formula-problem" role="alert">
            {error}
          </div>
        )}
      </div>
      <div className="formula-tools">
        <IconButton label={t("Format DAX")} shortcut="Shift+Alt+F" icon={<WandSparkles size={15} />} onPress={() => ref.current?.format()} />
        {column && <IconButton label={t("Delete column")} icon={<Trash2 size={15} />} onPress={remove} />}
        <IconButton label={t("Close formula bar")} icon={<X size={15} />} onPress={onDone} />
      </div>
    </div>
  );
}
