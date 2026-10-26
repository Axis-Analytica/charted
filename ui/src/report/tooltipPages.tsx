// Report-page tooltips: a page set up as a tooltip (usually small) is shown, filtered to
// the point under the pointer, when hovering a chart that uses it. A chart picks its
// tooltip page in the format pane, or by default (Automatic) uses the first tooltip page
// whose tooltip fields the point has, as in Power BI.
import { MessageSquare, X } from "lucide-react";
import { useState } from "react";
import { createPortal } from "react-dom";
import { create } from "zustand";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { QueryFilter } from "../bridge/generated/QueryFilter";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { useApp } from "../state/app";
import { dropFields, fieldKey, pageSettings, projectionName, sameField } from "../state/report";
import { FIELD_MIME } from "./Canvas";
import { ExtraFilters } from "./data";
import { TOOLTIP_PAGES } from "./moreVisuals";


export interface PageTip {
  /** The visual hovered, the tooltip page and the point's filters. */
  visual: string;
  page: string;
  filters: QueryFilter[];
  key: string;
  x: number;
  y: number;
}

export const usePageTip = create<{ tip: PageTip | null; show(t: PageTip): void; hide(visual: string): void }>((set, get) => ({
  tip: null,
  show: (tip) => {
    const cur = get().tip;
    if (cur && cur.key === tip.key && cur.visual === tip.visual && Math.abs(cur.x - tip.x) < 2 && Math.abs(cur.y - tip.y) < 2) return;
    set({ tip });
  },
  hide: (visual) => {
    if (get().tip?.visual === visual) set({ tip: null });
  },
}));

/** The visual's tooltip setting: "auto" (the default), "default" (the chart's own) or a page id. */
export function tooltipSetting(visual: Visual): string {
  const v = visual.objects?.tooltip?.page;
  return typeof v === "string" && v ? v : "auto";
}

/** The page shown as `visual`'s tooltip, if any. */
export function tooltipPageFor(model: Model, page: Page, visual: Visual): Page | null {
  if (!TOOLTIP_PAGES.includes(visual.visualType)) return null;
  const setting = tooltipSetting(visual);
  if (setting === "default") return null;
  const tips = model.report.pages.filter((p) => p.tooltip && p.id !== page.id);
  if (setting !== "auto") return tips.find((p) => p.id === setting) ?? null;
  const fields = Object.values(visual.roles).flatMap((ps) => ps ?? []);
  return tips.find((p) => (p.tooltipFields ?? []).length > 0 && (p.tooltipFields ?? []).every((f) => fields.some((x) => sameField(x.field, f.field)))) ?? null;
}

const equals = (field: Projection, value: string | null): QueryFilter => ({ field: { ...field, aggregation: null }, filter: { kind: "basic", values: [value], exclude: false } });

/** The filters a tooltip page shows a point under: the point's values, and the source
 * page's and visual's filters and slicers. */
export function tipFilters(from: Page, visual: Visual, fields: Projection[], tuple: (string | null)[]): QueryFilter[] {
  const filters = fields.map((f, i) => equals(f, tuple[i] ?? null));
  for (const f of [...from.filters, ...visual.filters]) if (f.filter.kind !== "all") filters.push({ field: f.field, filter: f.filter });
  for (const s of from.visuals) {
    const field = s.visualType === "slicer" ? s.roles.field?.[0] : undefined;
    if (field && s.selection && s.selection.kind !== "all" && s.id !== visual.id) filters.push({ field: { ...field, aggregation: null }, filter: s.selection });
  }
  return filters;
}

/** The hovered point's tooltip page, near the pointer, drawn by `renderVisual` for each
 * of its visuals (positioned at the page's own size). */
export function PageTooltip({ model, visual, render }: { model: Model; visual: string; render(page: Page, v: Visual): React.ReactNode }) {
  const tip = usePageTip((s) => (s.tip?.visual === visual ? s.tip : null));
  if (!tip) return null;
  const page = model.report.pages.find((p) => p.id === tip.page);
  if (!page) return null;
  // Keep it on screen: flip to the other side of the pointer near the edges.
  const w = page.width;
  const h = page.height;
  const x = tip.x + 16 + w > window.innerWidth ? Math.max(4, tip.x - 16 - w) : tip.x + 16;
  const y = tip.y + 16 + h > window.innerHeight ? Math.max(4, tip.y - 16 - h) : tip.y + 16;
  return createPortal(
    <ExtraFilters.Provider value={tip.filters}>
      <div className="page-tooltip" role="tooltip" aria-label={page.name} style={{ left: x, top: y, width: w, height: h }}>
        <div className="page-tooltip-page" style={{ width: w, height: h }}>
          {[...page.visuals]
            .sort((a, b) => a.position.z - b.position.z)
            .map((v) => (
              <div key={v.id} className="page-tooltip-visual" style={{ left: v.position.x, top: v.position.y, width: v.position.width, height: v.position.height }}>
                {render(page, v)}
              </div>
            ))}
        </div>
      </div>
    </ExtraFilters.Provider>,
    document.body,
  );
}

/** Page settings: use the page as a tooltip. */
export function TooltipPageSettings({ page }: { page: Page }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  return (
    <label className="check">
      <input
        type="checkbox"
        checked={!!page.tooltip}
        onChange={(e) => {
          const on = e.target.checked;
          edit("Use page as tooltip", (m) => {
            const p = m.report.pages.find((x) => x.id === page.id);
            if (!p) return null;
            // A new tooltip page takes Power BI's tooltip size.
            const size = on && p.width === 1280 && p.height === 720 ? { width: 320, height: 240 } : {};
            return [{ op: "setPage", page: p.id, settings: { ...pageSettings(p), tooltip: on, ...size } }];
          });
        }}
      />
      {t("Use as a tooltip")}
    </label>
  );
}

/** The Filters pane's Tooltip section on a tooltip page: its tooltip fields. */
export function TooltipFieldsSection({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const [over, setOver] = useState(false);
  if (!page.tooltip) return null;
  const setFields = (label: string, fn: (fields: Projection[]) => Projection[]) =>
    edit(label, (m) => {
      const p = m.report.pages.find((x) => x.id === page.id);
      return p ? [{ op: "setPage", page: p.id, settings: { ...pageSettings(p), tooltipFields: fn(p.tooltipFields ?? []) } }] : null;
    });
  return (
    <div className="filter-section" role="group" aria-label={t("Tooltip fields")}>
      <div className="filter-section-title">
        <MessageSquare size={12} /> {t("Tooltip fields")}
      </div>
      {(page.tooltipFields ?? []).map((d) => (
        <div key={fieldKey(d.field)} className="filter-card drill-field">
          <div className="filter-card-head">
            <span className="filter-card-name">{projectionName(model, d, t)}</span>
            <button type="button" className="filter-icon" aria-label={t("Remove %1", projectionName(model, d, t))} onClick={() => setFields("Remove tooltip field", (fs) => fs.filter((x) => fieldKey(x.field) !== fieldKey(d.field)))}>
              <X size={13} />
            </button>
          </div>
        </div>
      ))}
      <div
        className={`filter-drop ${over ? "is-over" : ""}`}
        onDragOver={(e) => {
          if (e.dataTransfer.types.includes(FIELD_MIME)) {
            e.preventDefault();
            setOver(true);
          }
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          const fields = dropFields(e.dataTransfer.getData(FIELD_MIME)).filter((f) => f.kind === "column" && !(page.tooltipFields ?? []).some((p) => sameField(p.field, f)));
          if (!fields.length) return;
          setFields("Add tooltip field", (fs) => [...fs, ...fields.map((field) => ({ field, aggregation: null, displayName: null }))]);
        }}
      >
        {t("Add tooltip fields here")}
      </div>
    </div>
  );
}

/** The format pane's Tooltips section: which page a chart shows as its tooltip. */
export function TooltipPagePicker({ model, page, visual, onChange }: { model: Model; page: Page; visual: Visual; onChange(v: string): void }) {
  const { t } = useT();
  const tips = model.report.pages.filter((p) => p.tooltip && p.id !== page.id);
  const used = tooltipPageFor(model, page, visual);
  return (
    <div className="format-body">
      <label className="field">
        <span className="field-label">{t("Report page")}</span>
        <select className="select" aria-label={t("Tooltip page")} value={tooltipSetting(visual)} onChange={(e) => onChange(e.target.value)}>
          <option value="auto">{t("Automatic")}</option>
          <option value="default">{t("Default tooltip")}</option>
          {tips.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </label>
      <p className="pane-hint">{used ? t("Showing the page “%1” on hover.", used.name) : tips.length ? t("No tooltip page matches this visual's fields.") : t("Set a page as a tooltip in its page settings to use it here.")}</p>
    </div>
  );
}
