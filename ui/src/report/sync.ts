// Synced slicers, as in Power BI: a slicer synced to other pages has a copy on each
// of them (visible or hidden), all in one sync group (`objects.syncGroup.groupName`).
// Each copy filters its own page, so the copies keep one choice: a change to one is
// made to all of them in the same undo step.
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Visual } from "../bridge/generated/Visual";

/** A slicer's sync group, or null. */
export function syncGroupOf(v: Visual | undefined): string | null {
  if (!v || v.visualType !== "slicer") return null;
  const name = (v.objects?.syncGroup as Record<string, unknown> | undefined)?.groupName;
  return typeof name === "string" && name ? name : null;
}

/** `v` in sync group `group` (none: out of any group). */
export function withSyncGroup(v: Visual, group: string | null): Visual {
  const objects = { ...(v.objects ?? {}) };
  if (group) objects.syncGroup = { ...(objects.syncGroup ?? {}), groupName: group };
  else delete objects.syncGroup;
  return { ...v, objects };
}

/** Every slicer in `group`, with its page. */
export function syncCopies(model: Model, group: string): { page: string; visual: Visual }[] {
  return model.report.pages.flatMap((p) => p.visuals.filter((v) => syncGroupOf(v) === group).map((visual) => ({ page: p.id, visual })));
}

/** The changes that carry a synced slicer's new choice (and field) to the other
 * copies in its group. `before` is the slicer as it was. */
export function syncChanges(model: Model, before: Visual | undefined, after: Visual): Change[] {
  const group = syncGroupOf(after);
  if (!group) return [];
  const selection = JSON.stringify(before?.selection ?? null) !== JSON.stringify(after.selection ?? null);
  const roles = JSON.stringify(before?.roles ?? null) !== JSON.stringify(after.roles);
  if (!selection && !roles) return [];
  return syncCopies(model, group).flatMap(({ page, visual: v }): Change[] => {
    if (v.id === after.id) return [];
    const next: Visual = { ...v, selection: after.selection ?? null, roles: roles ? after.roles : v.roles };
    return JSON.stringify(next) === JSON.stringify(v) ? [] : [{ op: "updateVisual", page, visual: next }];
  });
}

/** `changes` plus whatever keeps synced slicers' copies in step with them. */
export function withSync(model: Model, changes: Change[]): Change[] {
  const touched = new Set(changes.flatMap((c) => (c.op === "updateVisual" ? [c.visual.id] : [])));
  const extra = changes.flatMap((c) => {
    if (c.op !== "updateVisual") return [];
    const before = model.report.pages.find((p) => p.id === c.page)?.visuals.find((v) => v.id === c.visual.id);
    return syncChanges(model, before, c.visual).filter((x) => {
      if (x.op !== "updateVisual" || touched.has(x.visual.id)) return false;
      touched.add(x.visual.id);
      return true;
    });
  });
  return extra.length ? [...changes, ...extra] : changes;
}
