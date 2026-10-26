// The group dialog (Power BI's "New group"): bins for numbers and dates, by size or by
// a number of bins, and lists for text, where picked values become named groups with an
// optional group for everything else. It writes a calculated column (see groups.ts) and
// can be reopened from that column to change the groups.
import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../bridge";
import type { Change } from "../bridge/generated/Change";
import type { Column } from "../bridge/generated/Column";
import type { ColumnStats } from "../bridge/generated/ColumnStats";
import type { DistinctValues } from "../bridge/generated/DistinctValues";
import type { Model } from "../bridge/generated/Model";
import type { Table } from "../bridge/generated/Table";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";
import { newId } from "../state/report";
import { uniqueFieldName } from "./fieldTree";
import { type BinUnit, type GroupSpec, groupDax, groupKind, groupName, groupOf, isDateType, listGroupName, sizeForCount, withGroup } from "./groups";

/** The column to group, or the group column to edit. */
export type GroupTarget = { table: string; column: string };

export function GroupDialog({ model, target, onClose }: { model: Model; target: GroupTarget; onClose(): void }) {
  const table = model.tables.find((x) => x.id === target.table);
  const picked = table?.columns.find((x) => x.id === target.column);
  if (!table || !picked) return null;
  const spec = groupOf(picked);
  const source = spec ? table.columns.find((x) => x.id === spec.column) : picked;
  if (!source) return null;
  return <Group key={`${table.id}:${picked.id}`} table={table} source={source} existing={spec ? picked : null} onClose={onClose} />;
}

const UNITS: { unit: BinUnit; label: string }[] = [
  { unit: "day", label: "Days" },
  { unit: "month", label: "Months" },
  { unit: "year", label: "Years" },
];

function Group({ table, source, existing, onClose }: { table: Table; source: Column; existing: Column | null; onClose(): void }) {
  const { t } = useT();
  const kind = groupKind(source.dataType);
  const date = isDateType(source.dataType);
  const old = existing ? groupOf(existing) : null;
  const [name, setName] = useState(existing?.name ?? uniqueFieldName(table, groupName(source.name, kind)));
  const [unit, setUnit] = useState<BinUnit>(old?.kind === "bin" ? old.unit : date ? "month" : "number");
  const [by, setBy] = useState<"size" | "count">(old?.kind === "bin" ? old.by : "size");
  const [size, setSize] = useState(old?.kind === "bin" ? String(old.size) : date ? "1" : "10");
  const [count, setCount] = useState(old?.kind === "bin" ? String(old.count) : "10");
  const [groups, setGroups] = useState(old?.kind === "list" ? old.groups : []);
  const [other, setOther] = useState(old?.kind === "list" ? old.other !== null : true);
  const [otherName, setOtherName] = useState(old?.kind === "list" && old.other !== null ? old.other : t("Other"));
  const [checked, setChecked] = useState<string[]>([]);
  const [search, setSearch] = useState("");
  const [stats, setStats] = useState<ColumnStats | null>(null);
  const [values, setValues] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    if (kind === "bin")
      dataJson<ColumnStats>({ type: "stats", view: { table: table.id, sort: [], filters: [] }, column: source.id })
        .then((s) => live && setStats(s))
        .catch((e) => live && setError(errorMessage(e)));
    else
      dataJson<DistinctValues>({ type: "fieldValues", field: { field: { kind: "column", table: table.id, column: source.id }, aggregation: null, displayName: null }, filters: [], search: null, limit: 1000 })
        .then((d) => live && setValues(d.values.flatMap((v) => (v.key === null ? [] : [v.key]))))
        .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [kind, table.id, source.id]);

  // The size a number of bins gives over the column's range.
  const countSize = useMemo(() => {
    if (!stats) return null;
    const n = Number(count);
    if (!(n >= 1)) return null;
    if (date) {
      const days = stats.spanDays ?? 0;
      const span = unit === "day" ? days : unit === "month" ? days / 30.44 : days / 365.25;
      return sizeForCount(unit, n, 0, span, true);
    }
    const lo = Number(stats.min);
    const hi = Number(stats.max);
    return Number.isFinite(lo) && Number.isFinite(hi) ? sizeForCount("number", n, lo, hi, source.dataType === "Int64") : null;
  }, [stats, count, unit, date, source.dataType]);
  const binSize = by === "count" ? countSize : Number(size) > 0 ? Number(size) : null;

  const grouped = new Set(groups.flatMap((g) => g.values));
  const q = search.trim().toLowerCase();
  const ungrouped = (values ?? []).filter((v) => !grouped.has(v) && (!q || v.toLowerCase().includes(q)));
  const makeGroup = () => {
    if (!checked.length) return;
    setGroups((gs) => [...gs, { name: listGroupName(checked), values: checked }]);
    setChecked([]);
  };
  const ungroup = (i: number) => setGroups((gs) => gs.filter((_, j) => j !== i));
  const removeValue = (i: number, v: string) => setGroups((gs) => gs.map((g, j) => (j === i ? { ...g, values: g.values.filter((x) => x !== v) } : g)).filter((g) => g.values.length));

  const spec: GroupSpec | null =
    kind === "bin"
      ? binSize
        ? { kind: "bin", column: source.id, unit: date ? unit : "number", size: binSize, by, count: Number(count) || 10 }
        : null
      : { kind: "list", column: source.id, groups: groups.filter((g) => g.values.length && g.name.trim()).map((g) => ({ ...g, name: g.name.trim() })), other: other ? otherName.trim() || t("Other") : null };
  const invalid = !spec || !name.trim() || (spec.kind === "list" && groups.some((g) => !g.name.trim()));

  const save = async () => {
    if (!spec || invalid) return;
    setBusy(true);
    const expression = groupDax(spec, table.name, source.name, source.dataType);
    let changes: Change[];
    if (existing) {
      changes = [];
      if (name.trim() !== existing.name) changes.push({ op: "renameColumn", table: table.id, column: existing.id, name: name.trim() });
      changes.push({ op: "setColumn", table: table.id, column: existing.id, property: { name: "expression", value: expression } });
      changes.push({ op: "setColumn", table: table.id, column: existing.id, property: { name: "extras", value: withGroup(existing.extras, spec) } });
    } else {
      const column: Column = {
        id: newId(),
        name: name.trim(),
        dataType: "Unknown",
        kind: { kind: "calculated", expression, language: "Dax" },
        description: null,
        formatString: null,
        // Groups are categories: they go on axes, not into sums.
        summarizeBy: "None",
        dataCategory: null,
        sortBy: null,
        displayFolder: source.displayFolder ?? null,
        hidden: false,
        isKey: false,
        extras: withGroup({}, spec),
      };
      changes = [{ op: "addColumn", table: table.id, column, index: null }];
    }
    const ok = await useApp.getState().commit(existing ? "Edit groups" : "New group", changes);
    setBusy(false);
    if (ok) onClose();
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={existing ? t("Groups") : t("New group")}
      width={kind === "list" ? 620 : 440}
      footer={
        <>
          <Button variant="ghost" onPress={onClose}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy || invalid} onPress={save}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="quick-measure group-dialog">
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          <input className="input" aria-label={t("Name")} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        </label>
        <p className="field-hint">{t("Groups %1 into a new column of %2.", source.name, table.name)}</p>
        {kind === "bin" ? (
          <>
            <label className="field">
              <span className="field-label">{t("Bin type")}</span>
              <select className="select" aria-label={t("Bin type")} value={by} onChange={(e) => setBy(e.target.value as "size" | "count")}>
                <option value="size">{t("Size of bins")}</option>
                <option value="count">{t("Number of bins")}</option>
              </select>
            </label>
            {date && (
              <label className="field">
                <span className="field-label">{t("Unit")}</span>
                <select className="select" aria-label={t("Unit")} value={unit} onChange={(e) => setUnit(e.target.value as BinUnit)}>
                  {UNITS.map((u) => (
                    <option key={u.unit} value={u.unit}>
                      {t(u.label)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {by === "size" ? (
              <label className="field">
                <span className="field-label">{t("Bin size")}</span>
                <input className="input" type="number" min={date ? 1 : 0} step="any" aria-label={t("Bin size")} value={size} onChange={(e) => setSize(e.target.value)} />
              </label>
            ) : (
              <label className="field">
                <span className="field-label">{t("Number of bins")}</span>
                <input className="input" type="number" min={1} step={1} aria-label={t("Number of bins")} value={count} onChange={(e) => setCount(e.target.value)} />
              </label>
            )}
            {stats && (
              <p className="field-hint" aria-live="polite">
                {t("Values from %1 to %2.", String(stats.min ?? ""), String(stats.max ?? ""))} {binSize !== null && t("Bin size: %1", binSize)}
              </p>
            )}
          </>
        ) : (
          <div className="group-lists">
            <div className="group-col">
              <span className="field-label">{t("Ungrouped values")}</span>
              <input className="input" aria-label={t("Search values")} placeholder={t("Search")} value={search} onChange={(e) => setSearch(e.target.value)} />
              <ul className="group-values" aria-label={t("Ungrouped values")}>
                {values === null && !error && <li className="pane-hint">{t("Loading…")}</li>}
                {ungrouped.map((v) => (
                  <li key={v}>
                    <label className="check">
                      <input type="checkbox" checked={checked.includes(v)} onChange={(e) => setChecked((c) => (e.target.checked ? [...c, v] : c.filter((x) => x !== v)))} />
                      {v}
                    </label>
                  </li>
                ))}
              </ul>
              <Button size="sm" isDisabled={!checked.length} onPress={makeGroup}>
                {t("Group")}
              </Button>
            </div>
            <div className="group-col">
              <span className="field-label">{t("Groups and members")}</span>
              <ul className="group-values" aria-label={t("Groups and members")}>
                {groups.map((g, i) => (
                  <li key={i} className="group-item">
                    <div className="group-head">
                      <input className="input" aria-label={t("Group %1 name", i + 1)} value={g.name} onChange={(e) => setGroups((gs) => gs.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
                      <Button size="sm" variant="ghost" onPress={() => ungroup(i)}>
                        {t("Ungroup")}
                      </Button>
                    </div>
                    <ul>
                      {g.values.map((v) => (
                        <li key={v}>
                          <button type="button" className="group-member" title={t("Remove %1", v)} onClick={() => removeValue(i, v)}>
                            {v}
                          </button>
                        </li>
                      ))}
                    </ul>
                  </li>
                ))}
              </ul>
              <label className="check">
                <input type="checkbox" checked={other} onChange={(e) => setOther(e.target.checked)} />
                {t("Include Other group")}
              </label>
              {other && <input className="input" aria-label={t("Other group name")} value={otherName} onChange={(e) => setOtherName(e.target.value)} />}
            </div>
          </div>
        )}
        {error && <p className="field-error">{error}</p>}
      </div>
    </Dialog>
  );
}
