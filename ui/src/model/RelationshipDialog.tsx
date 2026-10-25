import { useEffect, useRef, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { Cardinality } from "../bridge/generated/Cardinality";
import type { Model } from "../bridge/generated/Model";
import type { Relationship } from "../bridge/generated/Relationship";
import type { RelationshipCheck } from "../bridge/generated/RelationshipCheck";
import { formatNumber, useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";

type Card = "manyToOne" | "oneToOne" | "oneToMany" | "manyToMany";

const toCard = (f: Cardinality, t: Cardinality): Card => (f === "Many" ? (t === "One" ? "manyToOne" : "manyToMany") : t === "One" ? "oneToOne" : "oneToMany");
const fromCard = (c: Card): [Cardinality, Cardinality] =>
  c === "manyToOne" ? ["Many", "One"] : c === "oneToOne" ? ["One", "One"] : c === "oneToMany" ? ["One", "Many"] : ["Many", "Many"];

/** Swaps the sides of a relationship, so the many side is always "from". */
function flipped(r: Relationship): Relationship {
  return { ...r, fromTable: r.toTable, fromColumn: r.toColumn, toTable: r.fromTable, toColumn: r.fromColumn, fromCardinality: r.toCardinality, toCardinality: r.fromCardinality };
}

export function RelationshipDialog({ model, initial, isNew, onClose }: { model: Model; initial: Relationship; isNew: boolean; onClose(): void }) {
  const { t } = useT();
  const [r, setR] = useState(initial);
  const [check, setCheck] = useState<RelationshipCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Cardinality follows the data until the user picks one.
  const chosen = useRef(!isNew);

  const ft = model.tables.find((x) => x.id === r.fromTable);
  const tt = model.tables.find((x) => x.id === r.toTable);
  const fc = ft?.columns.find((c) => c.id === r.fromColumn);
  const tc = tt?.columns.find((c) => c.id === r.toColumn);

  useEffect(() => {
    if (!fc || !tc) return;
    let live = true;
    setCheck(null);
    dataJson<RelationshipCheck>({ type: "checkRelationship", fromTable: r.fromTable, fromColumn: r.fromColumn, toTable: r.toTable, toColumn: r.toColumn })
      .then((c) => {
        if (!live) return;
        if (!chosen.current) {
          // Put the unique side on the right, as Power BI does.
          if (c.fromCardinality === "One" && c.toCardinality === "Many") {
            setR((cur) => flipped({ ...cur, fromCardinality: "One", toCardinality: "Many" }));
            return;
          }
          setR((cur) => ({ ...cur, fromCardinality: c.fromCardinality, toCardinality: c.toCardinality }));
        }
        setCheck(c);
      })
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [r.fromTable, r.fromColumn, r.toTable, r.toColumn, fc, tc]);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const change = isNew ? { op: "addRelationship" as const, relationship: r, index: null } : { op: "updateRelationship" as const, relationship: r };
      const state = await transport.rpc({ type: "commit", label: isNew ? "New relationship" : "Edit relationship", changes: [change] });
      useApp.setState({ state });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const remove = async () => {
    if (await useApp.getState().commit("Delete relationship", [{ op: "removeRelationship", relationship: r.id }])) onClose();
  };

  const n = (v: number) => formatNumber(v);
  const side = (label: string, table: string, column: string, onTable: (v: string) => void, onColumn: (v: string) => void) => {
    const tbl = model.tables.find((x) => x.id === table);
    return (
      <fieldset className="rel-side">
        <legend>{label}</legend>
        <label className="field">
          <span className="field-label">{t("Table")}</span>
          <select className="select" value={table} onChange={(e) => onTable(e.target.value)}>
            {model.tables.map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">{t("Column")}</span>
          <select className="select" value={column} onChange={(e) => onColumn(e.target.value)}>
            {tbl?.columns.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
      </fieldset>
    );
  };
  const firstColumn = (table: string) => model.tables.find((x) => x.id === table)?.columns[0]?.id ?? "";

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={isNew ? t("New relationship") : t("Edit relationship")}
      width={600}
      footer={
        <>
          {!isNew && (
            <Button variant="danger" onPress={remove}>
              {t("Delete")}
            </Button>
          )}
          <span className="dialog-spacer" />
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy || !fc || !tc || r.fromTable === r.toTable} onPress={save}>
            {isNew ? t("Create") : t("Save")}
          </Button>
        </>
      }
    >
      <div className="rel-form">
        <div className="rel-sides">
          {side(
            t("From (many side)"),
            r.fromTable,
            r.fromColumn,
            (v) => setR({ ...r, fromTable: v, fromColumn: firstColumn(v) }),
            (v) => setR({ ...r, fromColumn: v }),
          )}
          {side(
            t("To (one side)"),
            r.toTable,
            r.toColumn,
            (v) => setR({ ...r, toTable: v, toColumn: firstColumn(v) }),
            (v) => setR({ ...r, toColumn: v }),
          )}
        </div>
        <p className="rel-check" aria-live="polite">
          {check && ft && tt && fc && tc
            ? [
                t("%1[%2]: %3 distinct of %4 rows", ft.name, fc.name, n(check.fromDistinct), n(check.fromRows)),
                t("%1[%2]: %3 distinct of %4 rows", tt.name, tc.name, n(check.toDistinct), n(check.toRows)),
                check.unmatched ? t("%1 keys have no match", n(check.unmatched)) : t("every key has a match"),
              ].join(" · ")
            : " "}
        </p>
        <div className="rel-options">
          <label className="field">
            <span className="field-label">{t("Cardinality")}</span>
            <select
              className="select"
              value={toCard(r.fromCardinality, r.toCardinality)}
              onChange={(e) => {
                chosen.current = true;
                const [f, to] = fromCard(e.target.value as Card);
                setR({ ...r, fromCardinality: f, toCardinality: to });
              }}
            >
              <option value="manyToOne">{t("Many to one (*:1)")}</option>
              <option value="oneToOne">{t("One to one (1:1)")}</option>
              <option value="oneToMany">{t("One to many (1:*)")}</option>
              <option value="manyToMany">{t("Many to many (*:*)")}</option>
            </select>
          </label>
          <label className="field">
            <span className="field-label">{t("Cross filter direction")}</span>
            <select className="select" value={r.crossFilter} onChange={(e) => setR({ ...r, crossFilter: e.target.value as Relationship["crossFilter"] })}>
              <option value="Single">{t("Single")}</option>
              <option value="Both">{t("Both")}</option>
            </select>
          </label>
          <label className="check rel-active">
            <input type="checkbox" checked={r.active} onChange={(e) => setR({ ...r, active: e.target.checked })} />
            {t("Make this relationship active")}
          </label>
        </div>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
