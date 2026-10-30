// Calculation groups' pure logic (the dialog is CalculationGroupDialog.tsx).
import type { CalculationGroup } from "../bridge/generated/CalculationGroup";
import type { CalculationItem } from "../bridge/generated/CalculationItem";
import type { Table } from "../bridge/generated/Table";
import { msg } from "../i18n";

/** What's wrong with a group's items, as a message to translate, or null. */
export function itemsProblem(items: CalculationItem[]): string | null {
  if (!items.length) return msg("Add at least one item.");
  const seen = new Set<string>();
  for (const it of items) {
    const name = it.name.trim();
    if (!name) return msg("Every item needs a name.");
    if (seen.has(name.toLowerCase())) return msg("Two items can't have the same name.");
    seen.add(name.toLowerCase());
    if (!it.expression.trim()) return msg("Every item needs an expression.");
  }
  return null;
}

/** Items as they are saved: names trimmed, empty format strings left out. */
export function cleanItems(items: CalculationItem[]): CalculationItem[] {
  return items.map((it) => ({
    ...it,
    name: it.name.trim(),
    expression: it.expression.trim(),
    formatString: it.formatString?.trim() || null,
  }));
}

/** A new calculation group's table. The core gives it its Name and (hidden) Ordinal
 * columns and its rows. */
export function calculationGroupTable(name: string, group: CalculationGroup, newId: () => string): Table {
  return {
    id: newId(),
    name,
    description: null,
    source: "",
    columns: [],
    measures: [],
    hierarchies: [],
    hidden: false,
    isDateTable: false,
    calculationGroup: { ...group, items: cleanItems(group.items) },
  };
}

/** A new item's expression: the measure as it is. */
export const SELECTED_MEASURE = "SELECTEDMEASURE ()";
