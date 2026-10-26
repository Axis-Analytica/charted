// The Sync slicers pane: for the selected slicer, each page with two checkboxes, as in
// Power BI. Sync puts a copy of the slicer on that page (hidden unless Visible is also
// ticked) in the slicer's sync group, so one choice filters every synced page.
// Unticking Sync removes a hidden copy, or takes a visible one out of the group.
import { Eye, RefreshCw } from "lucide-react";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { useApp } from "../state/app";
import { newId, projectionName, updateVisual, useReport } from "../state/report";
import { syncCopies, syncGroupOf, withSyncGroup } from "./sync";
import "./building.css";

/** A group name not used by another group: the slicer's field name, numbered if need be. */
function freshGroup(model: Model, base: string): string {
  const used = new Set(model.report.pages.flatMap((p) => p.visuals.map(syncGroupOf)).filter(Boolean));
  let name = base || "Slicer";
  for (let n = 2; used.has(name); n++) name = `${base} ${n}`;
  return name;
}

export function SyncSlicersPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const selected = useReport((s) => s.selected);
  const slicer = selected.length === 1 ? page.visuals.find((v) => v.id === selected[0] && v.visualType === "slicer") : undefined;
  const group = syncGroupOf(slicer);
  const copyOn = (p: Page): Visual | undefined => (p.id === page.id ? slicer : group ? p.visuals.find((v) => syncGroupOf(v) === group) : undefined);

  /** Builds changes from the latest model, with the slicer as it is there. */
  const change = (label: string, fn: (m: Model, s: Visual, group: string) => Change[]) =>
    edit(label, (m) => {
      const s = m.report.pages.find((p) => p.id === page.id)?.visuals.find((v) => v.id === slicer?.id);
      if (!s) return null;
      const g = syncGroupOf(s);
      if (g) return fn(m, s, g);
      // The first page synced starts the group, named after the field.
      const field = s.roles.field?.[0];
      const named = freshGroup(m, field ? projectionName(m, field) : t("Slicer"));
      const grouped = withSyncGroup(s, named);
      return [updateVisual(page.id, grouped), ...fn(m, grouped, named)];
    });

  const addCopy = (s: Visual, g: string, to: Page, hidden: boolean): Change => {
    const top = to.visuals.reduce((z, v) => Math.max(z, v.position.z), 0);
    const width = Math.min(s.position.width, to.width);
    const height = Math.min(s.position.height, to.height);
    const copy: Visual = {
      ...withSyncGroup(s, g),
      id: newId(),
      hidden,
      filters: s.filters.map((f) => ({ ...f, id: newId() })),
      position: { x: Math.min(s.position.x, to.width - width), y: Math.min(s.position.y, to.height - height), width, height, z: top + 1 },
    };
    return { op: "addVisual", page: to.id, visual: copy, index: null };
  };

  const setSync = (to: Page, on: boolean) =>
    change(on ? "Sync slicer" : "Stop syncing slicer", (m, s, g) => {
      const target = m.report.pages.find((p) => p.id === to.id);
      if (!target) return [];
      const copy = target.visuals.find((v) => syncGroupOf(v) === g);
      if (on) return copy ? [] : [addCopy(s, g, target, true)];
      if (!copy) return [];
      return copy.hidden ? [{ op: "removeVisual", page: target.id, visual: copy.id }] : [updateVisual(target.id, withSyncGroup(copy, null))];
    });

  const setVisible = (to: Page, on: boolean) =>
    to.id === page.id
      ? edit(on ? "Show slicer" : "Hide slicer", (m) => {
          const s = m.report.pages.find((p) => p.id === page.id)?.visuals.find((v) => v.id === slicer?.id);
          return s && !!s.hidden === on ? [updateVisual(page.id, { ...s, hidden: !on })] : null;
        })
      : change(on ? "Show slicer" : "Hide slicer", (m, s, g) => {
          const target = m.report.pages.find((p) => p.id === to.id);
          if (!target) return [];
          const copy = target.visuals.find((v) => syncGroupOf(v) === g);
          if (!copy) return on ? [addCopy(s, g, target, false)] : [];
          return !!copy.hidden === !on ? [] : [updateVisual(target.id, { ...copy, hidden: !on })];
        });

  const rename = (name: string) =>
    edit("Rename sync group", (m) => {
      const s = m.report.pages.find((p) => p.id === page.id)?.visuals.find((v) => v.id === slicer?.id);
      const g = syncGroupOf(s);
      if (!s || !name || name === g) return null;
      const members = g ? syncCopies(m, g) : [{ page: page.id, visual: s }];
      return members.map(({ page: p, visual }) => updateVisual(p, withSyncGroup(visual, name)));
    });

  return (
    <section className="pane sync-pane" aria-label={t("Sync slicers")}>
      <div className="pane-head">
        <span>{t("Sync slicers")}</span>
      </div>
      <div className="pane-body">
        {!slicer ? (
          <p className="pane-hint">{t("Select a slicer to sync it to other pages, so one choice filters all of them.")}</p>
        ) : (
          <>
            <table className="sync-table" aria-label={t("Pages")}>
              <thead>
                <tr>
                  <th>{t("Page")}</th>
                  <th title={t("Sync")}>
                    <RefreshCw size={13} aria-label={t("Sync")} />
                  </th>
                  <th title={t("Visible")}>
                    <Eye size={13} aria-label={t("Visible")} />
                  </th>
                </tr>
              </thead>
              <tbody>
                {model.report.pages.map((p) => {
                  const copy = copyOn(p);
                  const here = p.id === page.id;
                  return (
                    <tr key={p.id} className={here ? "is-current" : ""} data-page={p.id}>
                      <td className="sync-page">{p.name}</td>
                      <td>
                        <input type="checkbox" aria-label={t("Sync to %1", p.name)} checked={!!copy} disabled={here} onChange={(e) => setSync(p, e.target.checked)} />
                      </td>
                      <td>
                        <input type="checkbox" aria-label={t("Visible on %1", p.name)} checked={!!copy && !copy.hidden} onChange={(e) => setVisible(p, e.target.checked)} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <details className="sync-advanced">
              <summary>{t("Advanced options")}</summary>
              <label className="field">
                <span className="field-label">{t("Group name")}</span>
                <input
                  className="input"
                  key={group ?? ""}
                  defaultValue={group ?? ""}
                  placeholder={t("Not synced")}
                  onBlur={(e) => rename(e.target.value.trim())}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                  }}
                />
              </label>
              <p className="pane-hint">{t("Slicers with the same group name share one choice on every page they're on.")}</p>
            </details>
          </>
        )}
      </div>
    </section>
  );
}
