// The buttons and menu items people can hide (app/layout.ts keeps the hidden ids per
// profile): the Layout menu lists them, and the agent's list_commands and hide_commands
// name them by these ids. Hidden ones stay in the command palette (Ctrl+K).
import type { MenuEntry } from "../kit/Menu";
import { msg } from "../i18n";
import { TOOLBAR_COMMANDS } from "../report/toolbar";

export interface Hideable {
  id: string;
  label: string;
  /** Where it is, as the Layout menu groups them. */
  place: string;
}

const at = (place: string, items: { id: string; label: string }[]): Hideable[] => items.map((c) => ({ ...c, place }));

export const HIDEABLE: Hideable[] = [
  ...at(msg("Report toolbar"), [...TOOLBAR_COMMANDS, { id: "theme", label: msg("Theme") }, { id: "security", label: msg("Security") }]),
  ...at(msg("File menu"), [
    { id: "file-new", label: msg("New project…") },
    { id: "file-save-bundle", label: msg("Save as bundle…") },
    { id: "file-import-pbip", label: msg("Import Power BI project…") },
    { id: "file-export-pbip", label: msg("Export Power BI project…") },
    { id: "file-export-ossie", label: msg("Export Apache Ossie model…") },
    { id: "file-export-pdf", label: msg("Export to PDF…") },
    { id: "file-import-ossie", label: msg("Import Apache Ossie model…") },
    { id: "file-report-problem", label: msg("Report a problem…") },
    { id: "file-discussions", label: msg("Ideas and questions…") },
  ]),
  ...at(msg("Data view toolbar"), [
    { id: "data-new-column", label: msg("New column") },
    { id: "data-find", label: msg("Find in table") },
    { id: "data-stats", label: msg("Column statistics") },
    { id: "data-props", label: msg("Column properties") },
  ]),
  ...at(msg("Model view toolbar"), [
    { id: "model-new-relationship", label: msg("New relationship") },
    { id: "model-date-table", label: msg("New date table") },
    { id: "model-arrange", label: msg("Arrange") },
    { id: "model-zoom", label: msg("Zoom") },
    { id: "model-fit", label: msg("Fit diagram") },
  ]),
  ...at(msg("Transform data toolbar"), [
    { id: "query-new-source", label: msg("New source") },
    ...["Manage columns", "Reduce rows", "Transform", "Add column", "Combine"].map((g) => ({ id: `query-${g.toLowerCase().replace(/ /g, "-")}`, label: g })),
    { id: "query-undo", label: msg("Undo in the query editor") },
    { id: "query-redo", label: msg("Redo in the query editor") },
  ]),
];

export const hideable = (id: string) => HIDEABLE.find((c) => c.id === id);

/** A menu's items without the hidden ones (and without separators left doubled or at an end). */
export function shownItems(items: (MenuEntry | null)[], hidden: string[]): (MenuEntry | null)[] {
  const out: (MenuEntry | null)[] = [];
  for (const i of items) {
    if (i && i.command && hidden.includes(i.command)) continue;
    if (!i && (!out.length || !out[out.length - 1])) continue;
    out.push(i);
  }
  while (out.length && !out[out.length - 1]) out.pop();
  return out;
}
