// The Bookmarks pane: saved views of the report. A bookmark keeps a page with the
// report's and the page's filters and each visual's filters, slicer choice, sort and
// drill state; showing it puts them back (one undoable step). Ctrl+Alt+→ and ← step
// through the bookmarks; in the list, Enter shows one, F2 renames, Delete removes and
// Alt+↑/↓ move.
import { Bookmark as BookmarkIcon, ChevronDown, ChevronUp, MoreHorizontal, Pencil, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { create } from "zustand";
import type { Bookmark } from "../bridge/generated/Bookmark";
import type { Change } from "../bridge/generated/Change";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { VisualState } from "../bridge/generated/VisualState";
import { useShortcut } from "../app/shortcuts";
import { useT } from "../i18n";
import { IconButton } from "../kit/Button";
import { Menu } from "../kit/Menu";
import { useApp } from "../state/app";
import { useDrillThrough } from "../state/drillthrough";
import { newId, pageSettings, updateVisual, useReport } from "../state/report";
import { useSelection } from "../state/selection";
import { withSync } from "./sync";

/** The bookmark shown last (for next and previous). */
export const useBookmark = create<{ current: string | null; set(id: string | null): void }>((set) => ({ current: null, set: (current) => set({ current }) }));

const stateOf = (v: Page["visuals"][number]): VisualState => ({ filters: v.filters, selection: v.selection ?? null, sort: v.sort ?? null, drill: v.drill ?? [] });

/** The current view of `page` as a bookmark. */
export function capture(model: Model, page: Page, name: string, id = newId()): Bookmark {
  return {
    id,
    name,
    page: page.id,
    reportFilters: model.report.filters,
    pageFilters: page.filters,
    visuals: Object.fromEntries(page.visuals.map((v) => [v.id, stateOf(v)])),
  };
}

/** Shows a bookmark: its page, with its filters and visual states put back. */
export async function showBookmark(model: Model, b: Bookmark) {
  const page = model.report.pages.find((p) => p.id === b.page);
  if (!page) return;
  const changes: Change[] = [];
  if (JSON.stringify(model.report.filters) !== JSON.stringify(b.reportFilters)) changes.push({ op: "setReportFilters", filters: b.reportFilters });
  if (JSON.stringify(page.filters) !== JSON.stringify(b.pageFilters)) changes.push({ op: "setPage", page: page.id, settings: { ...pageSettings(page), filters: b.pageFilters } });
  for (const v of page.visuals) {
    const s = b.visuals[v.id];
    if (s && JSON.stringify(stateOf(v)) !== JSON.stringify(s)) changes.push(updateVisual(page.id, { ...v, filters: s.filters, selection: s.selection ?? null, sort: s.sort ?? null, drill: s.drill ?? [] }));
  }
  useSelection.getState().clear();
  useDrillThrough.getState().set(null);
  useReport.getState().set({ page: page.id, selected: [] });
  useBookmark.getState().set(b.id);
  if (changes.length) await useApp.getState().commit("Show bookmark", withSync(model, changes));
}

const setBookmarks = (label: string, fn: (bs: Bookmark[], model: Model) => Bookmark[]) => useApp.getState().edit(label, (m) => [{ op: "setBookmarks", bookmarks: fn(m.report.bookmarks ?? [], m) }]);

/** Shows the next (or previous) bookmark. */
export function stepBookmark(model: Model, by: 1 | -1) {
  const bs = model.report.bookmarks ?? [];
  if (!bs.length) return;
  const i = bs.findIndex((b) => b.id === useBookmark.getState().current);
  const next = bs[i < 0 ? (by > 0 ? 0 : bs.length - 1) : (i + by + bs.length) % bs.length]!;
  void showBookmark(model, next);
}

/** Ctrl+Alt+→ and ← step through the bookmarks (with or without the pane). */
export function useBookmarkShortcuts(model: Model) {
  const has = (model.report.bookmarks ?? []).length > 0;
  useShortcut(
    { id: "bookmark-next", keys: "Ctrl+Alt+ArrowRight", label: "Next bookmark", group: "View", run: () => stepBookmark(model, 1), enabled: () => has },
    { id: "bookmark-previous", keys: "Ctrl+Alt+ArrowLeft", label: "Previous bookmark", group: "View", run: () => stepBookmark(model, -1), enabled: () => has },
  );
}

export function BookmarksPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const bookmarks = model.report.bookmarks ?? [];
  const current = useBookmark((s) => s.current);
  const [renaming, setRenaming] = useState<string | null>(null);
  const list = useRef<HTMLUListElement>(null);

  const add = () => {
    const names = new Set(bookmarks.map((b) => b.name));
    let n = bookmarks.length + 1;
    while (names.has(t("Bookmark %1", n))) n++;
    const b = capture(model, page, t("Bookmark %1", n));
    void setBookmarks("Add bookmark", (bs) => [...bs, b]);
    useBookmark.getState().set(b.id);
  };
  // Update: the bookmark takes the current view (this page as it is now).
  const update = (b: Bookmark) => void setBookmarks("Update bookmark", (bs, m) => bs.map((x) => (x.id === b.id ? capture(m, m.report.pages.find((y) => y.id === page.id) ?? page, b.name, b.id) : x)));
  const remove = (b: Bookmark) => void setBookmarks("Delete bookmark", (bs) => bs.filter((x) => x.id !== b.id));
  const move = (b: Bookmark, by: number) =>
    void setBookmarks("Move bookmark", (bs) => {
      const i = bs.findIndex((x) => x.id === b.id);
      const j = i + by;
      if (i < 0 || j < 0 || j >= bs.length) return bs;
      const next = [...bs];
      [next[i], next[j]] = [next[j]!, next[i]!];
      return next;
    });
  const rename = (b: Bookmark, name: string) => {
    setRenaming(null);
    if (name.trim() && name.trim() !== b.name) void setBookmarks("Rename bookmark", (bs) => bs.map((x) => (x.id === b.id ? { ...x, name: name.trim() } : x)));
  };
  const focusAt = (i: number) => requestAnimationFrame(() => list.current?.querySelectorAll<HTMLElement>(".bookmark-name")[i]?.focus());

  return (
    <section className="pane bookmarks-pane" aria-label={t("Bookmarks")}>
      <div className="pane-head">
        <span>{t("Bookmarks")}</span>
        <IconButton label={t("Add bookmark")} icon={<Plus size={14} />} onPress={add} />
      </div>
      <div className="pane-body">
        {bookmarks.length === 0 ? (
          <p className="pane-hint">{t("Add a bookmark to save this view: the page, its filters, slicers, sorting and drill state.")}</p>
        ) : (
          <ul ref={list} className="bookmark-list" aria-label={t("Bookmarks")}>
            {bookmarks.map((b, i) => (
              <li key={b.id} className={`bookmark ${current === b.id ? "is-on" : ""}`}>
                {renaming === b.id ? (
                  <input
                    className="input bookmark-input"
                    defaultValue={b.name}
                    aria-label={t("Bookmark name")}
                    // biome-ignore lint/a11y/noAutofocus: renaming starts in the field
                    autoFocus
                    onFocus={(e) => e.target.select()}
                    onBlur={(e) => rename(b, e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") rename(b, e.currentTarget.value);
                      if (e.key === "Escape") setRenaming(null);
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    className="bookmark-name"
                    aria-current={current === b.id}
                    title={model.report.pages.find((p) => p.id === b.page)?.name}
                    onClick={() => showBookmark(model, b)}
                    onKeyDown={(e) => {
                      if (e.key === "F2") (e.preventDefault(), setRenaming(b.id));
                      else if (e.key === "Delete") (e.preventDefault(), remove(b), focusAt(Math.max(0, i - 1)));
                      else if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                        e.preventDefault();
                        move(b, e.key === "ArrowUp" ? -1 : 1);
                        focusAt(i + (e.key === "ArrowUp" ? -1 : 1));
                      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") (e.preventDefault(), focusAt(i + (e.key === "ArrowDown" ? 1 : -1)));
                    }}
                  >
                    <BookmarkIcon size={13} />
                    <span>{b.name}</span>
                  </button>
                )}
                <Menu
                  label={t("Options for %1", b.name)}
                  trigger={<MoreHorizontal size={14} />}
                  items={[
                    { id: "update", label: t("Update"), icon: <RefreshCw size={13} />, run: () => update(b) },
                    { id: "rename", label: t("Rename"), icon: <Pencil size={13} />, run: () => setRenaming(b.id) },
                    { id: "up", label: t("Move up"), icon: <ChevronUp size={13} />, run: () => move(b, -1), disabled: i === 0 },
                    { id: "down", label: t("Move down"), icon: <ChevronDown size={13} />, run: () => move(b, 1), disabled: i === bookmarks.length - 1 },
                    { id: "delete", label: t("Delete"), icon: <Trash2 size={13} />, run: () => remove(b) },
                  ]}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
