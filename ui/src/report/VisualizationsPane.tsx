// The Visualizations pane: the visual gallery, field wells (Build) and the formatting
// options generated from the registry (Format). With no visual selected, Format shows
// the page's settings.
import { ChevronDown, GripVertical, Search, X } from "lucide-react";
import { useRef, useState } from "react";
import type { Aggregation } from "../bridge/generated/Aggregation";
import type { FieldRef } from "../bridge/generated/FieldRef";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { Projection } from "../bridge/generated/Projection";
import type { Visual } from "../bridge/generated/Visual";
import { useT } from "../i18n";
import { useApp } from "../state/app";
import { aggregationLabel, columnOf, editVisual, newVisual, pageSettings, projectionName, updateVisual, useReport, withField, withoutField } from "../state/report";
import { FIELD_MIME, VISUAL_MIME } from "./Canvas";
import { FormatEditor } from "./formatEditors";
import { TooltipPageSettings } from "./tooltipPages";
import { commonObjects, MIXED, shownValue, withProp } from "./formatting";
import { withSync } from "./sync";
import { type ObjectDef, type PropDef, type RoleDef, VISUAL_KEYWORDS, VISUALS, visualDef } from "./registry";
import { palette } from "./visuals/common";

const CHIP_MIME = "application/x-charted-chip";
const AGGREGATIONS: Aggregation[] = ["sum", "average", "min", "max", "count", "distinctCount"];

/** The same fields in a visual of another type. */
function convert(model: Model, v: Visual, type: string): Visual {
  let next: Visual = { ...v, visualType: type, roles: {}, sort: null };
  // Fields keep a role the new type also has (a combo's line values, small multiples).
  const def = visualDef(type);
  const rest: Projection[] = [];
  for (const [role, ps] of Object.entries(v.roles))
    for (const p of ps ?? []) {
      const r = def?.roles.find((x) => x.name === role);
      const list = next.roles[role] ?? [];
      if (r && (r.max === undefined || list.length < r.max) && !(p.field.kind === "measure" && r.kind === "grouping")) next = { ...next, roles: { ...next.roles, [role]: [...list, p] } };
      else rest.push(p);
    }
  for (const p of rest) next = withField(model, next, p.field) ?? next;
  return next;
}

export function VisualizationsPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const { selected, select, vizTab, set } = useReport();
  const visual = selected.length === 1 ? page.visuals.find((v) => v.id === selected[0]) : undefined;
  // Format edits every selected visual together.
  const formatted = page.visuals.filter((v) => selected.includes(v.id));
  const [search, setSearch] = useState("");
  const gallery = useRef<HTMLDivElement>(null);
  const q = search.trim().toLowerCase();
  const shown = q ? VISUALS.filter((d) => [t(d.label), d.label, d.type, ...(VISUAL_KEYWORDS[d.type] ?? [])].some((w) => w.toLowerCase().includes(q))) : VISUALS;

  const pick = async (type: string) => {
    if (visual) {
      if (visual.visualType !== type) await commit("Change visual type", [updateVisual(page.id, convert(model, visual, type))]);
      return;
    }
    const v = newVisual(page, type, useReport.getState().pointer);
    if (await commit("Add visual", [{ op: "addVisual", page: page.id, visual: v, index: null }])) select([v.id]);
  };

  return (
    <section className="pane viz-pane" aria-label={t("Visualizations")}>
      <div className="pane-head">
        <span>{t("Visualizations")}</span>
      </div>
      <label className="pane-search">
        <Search size={13} />
        <input
          value={search}
          placeholder={t("Search visuals")}
          aria-label={t("Search visuals")}
          onChange={(e) => setSearch(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && shown[0]) {
              e.preventDefault();
              pick(shown[0].type);
              setSearch("");
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              gallery.current?.querySelector<HTMLButtonElement>(".viz-type")?.focus();
            } else if (e.key === "Escape" && search) {
              e.stopPropagation();
              setSearch("");
            }
          }}
        />
      </label>
      <div
        ref={gallery}
        className="viz-gallery"
        role="toolbar"
        aria-label={t("Visual types")}
        onKeyDown={(e) => {
          // Typing in the gallery searches it.
          if (e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey) e.currentTarget.previousElementSibling?.querySelector("input")?.focus();
        }}
      >
        {shown.length === 0 && <p className="pane-hint viz-none">{t("No visual matches “%1”.", search)}</p>}
        {shown.map((d) => (
          <button
            key={d.type}
            type="button"
            className={`viz-type ${visual?.visualType === d.type ? "is-on" : ""}`}
            aria-label={t(d.label)}
            aria-pressed={visual?.visualType === d.type}
            title={t(d.label)}
            draggable
            onDragStart={(e) => e.dataTransfer.setData(VISUAL_MIME, d.type)}
            onClick={() => pick(d.type)}
          >
            <d.icon size={18} />
          </button>
        ))}
      </div>
      <div className="pane-tabs" role="tablist">
        <button type="button" role="tab" aria-selected={vizTab === "build"} onClick={() => set({ vizTab: "build" })}>
          {t("Build")}
        </button>
        <button type="button" role="tab" aria-selected={vizTab === "format"} onClick={() => set({ vizTab: "format" })}>
          {t("Format")}
        </button>
      </div>
      <div className="pane-body">
        {vizTab === "build" ? (
          visual ? (
            <Wells model={model} page={page} visual={visual} />
          ) : (
            <p className="pane-hint">{selected.length > 1 ? t("Select a single visual to edit its fields.") : t("Select a visual, or pick a type above to add one.")}</p>
          )
        ) : formatted.length ? (
          <FormatPane page={page} visuals={formatted} />
        ) : (
          <PageFormat page={page} />
        )}
      </div>
    </section>
  );
}

function Wells({ model, page, visual }: { model: Model; page: Page; visual: Visual }) {
  const { t } = useT();
  const def = visualDef(visual.visualType);
  if (!def) return null;
  return (
    <div className="wells">
      {def.roles.map((r) => (
        <Well key={r.name} model={model} page={page} visual={visual} role={r} />
      ))}
      <p className="pane-hint">{t("Drag fields here, or tick them in the Fields pane.")}</p>
    </div>
  );
}

function Well({ model, page, visual, role }: { model: Model; page: Page; visual: Visual; role: RoleDef }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const [over, setOver] = useState(false);
  const list = visual.roles[role.name] ?? [];
  const full = role.max !== undefined && list.length >= role.max;

  const drop = async (e: React.DragEvent, index?: number) => {
    e.preventDefault();
    setOver(false);
    const chip = e.dataTransfer.getData(CHIP_MIME);
    if (chip) {
      const from = JSON.parse(chip) as { role: string; index: number };
      await edit(
        "Move field",
        editVisual(page.id, visual.id, (v, m) => {
          const p = v.roles[from.role]?.[from.index];
          if (!p) return null;
          let next = withoutField(v, from.role, from.index);
          const at = from.role === role.name && index !== undefined && index > from.index ? index - 1 : index;
          next = withField(m, next, p.field, role.name, at) ?? next;
          // Keep the chip's own summarisation when it stays a value.
          const moved = next.roles[role.name]?.[at ?? (next.roles[role.name]?.length ?? 1) - 1];
          if (moved && p.aggregation && role.kind !== "grouping") moved.aggregation = p.aggregation;
          return next;
        }),
      );
      return;
    }
    const raw = e.dataTransfer.getData(FIELD_MIME);
    if (!raw) return;
    const field = JSON.parse(raw) as FieldRef;
    await edit("Add field", editVisual(page.id, visual.id, (v, m) => withField(m, v, field, role.name, index)));
  };

  const setProjection = (i: number, p: Projection) =>
    edit(
      "Change field",
      editVisual(page.id, visual.id, (v) => ({ ...v, roles: { ...v.roles, [role.name]: (v.roles[role.name] ?? []).map((x, j) => (j === i ? p : x)) } })),
    );

  return (
    <div
      className={`well ${over ? "is-over" : ""}`}
      data-role={role.name}
      onDragOver={(e) => {
        const types = e.dataTransfer.types;
        if (types.includes(FIELD_MIME) || types.includes(CHIP_MIME)) {
          e.preventDefault();
          setOver(true);
        }
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => drop(e)}
    >
      <div className="well-label">{t(role.label)}</div>
      <ul className="well-list" aria-label={t(role.label)}>
        {list.map((p, i) => {
          const col = columnOf(model, p.field);
          const canSummarise = !!col && role.kind !== "grouping";
          return (
            <li
              key={i}
              className="chip-field"
              draggable
              onDragStart={(e) => e.dataTransfer.setData(CHIP_MIME, JSON.stringify({ role: role.name, index: i }))}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.stopPropagation();
                drop(e, i);
              }}
            >
              <GripVertical size={12} className="chip-grip" />
              <span className="chip-name" title={projectionName(model, p, t)}>
                {projectionName(model, p, t)}
              </span>
              {canSummarise && (
                <span className="chip-agg">
                  <select
                    aria-label={t("Summarize %1", col.name)}
                    value={p.aggregation ?? ""}
                    onChange={(e) => setProjection(i, { ...p, aggregation: (e.target.value || null) as Aggregation | null })}
                  >
                    {role.kind === "any" && <option value="">{t("Don't summarize")}</option>}
                    {AGGREGATIONS.filter((a) => col.dataType === "Int64" || col.dataType === "Double" || col.dataType === "Decimal" || ["min", "max", "count", "distinctCount"].includes(a)).map((a) => (
                      <option key={a} value={a}>
                        {t(aggregationLabel(a))}
                      </option>
                    ))}
                  </select>
                  <ChevronDown size={11} />
                </span>
              )}
              <button type="button" className="chip-remove" aria-label={t("Remove %1", projectionName(model, p, t))} onClick={() => edit("Remove field", editVisual(page.id, visual.id, (v) => withoutField(v, role.name, i)))}>
                <X size={12} />
              </button>
            </li>
          );
        })}
        {!full && <li className="well-empty">{t("Add data fields here")}</li>}
      </ul>
    </div>
  );
}

function Control({ p, value: shown, onChange }: { p: PropDef; value: unknown; onChange(v: unknown): void }) {
  const { t } = useT();
  // Several visuals with different values show as mixed until one is chosen for all.
  const mixed = shown === MIXED;
  const value = mixed ? undefined : shown;
  switch (p.type) {
    case "bool":
      return (
        <label className="check">
          <input type="checkbox" checked={!!value} ref={(el) => void (el && (el.indeterminate = mixed))} onChange={(e) => onChange(e.target.checked)} />
          {t(p.label)}
        </label>
      );
    case "text":
      return (
        <label className="field">
          <span className="field-label">{t(p.label)}</span>
          <input
            className="input"
            key={mixed ? "mixed" : String(value)}
            defaultValue={String(value ?? "")}
            placeholder={mixed ? t("Mixed") : p.placeholder ? t(p.placeholder) : undefined}
            onBlur={(e) => (mixed ? e.target.value !== "" : e.target.value !== (value ?? "")) && onChange(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            }}
          />
        </label>
      );
    case "number":
      return (
        <label className="field">
          <span className="field-label">{t(p.label)}</span>
          <input
            className="input"
            type="number"
            key={mixed ? "mixed" : String(value)}
            min={p.min}
            max={p.max}
            step={p.step ?? 1}
            defaultValue={mixed ? "" : Number(value)}
            placeholder={mixed ? t("Mixed") : undefined}
            onBlur={(e) => {
              if (e.target.value === "") return;
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n !== value) onChange(Math.min(p.max ?? n, Math.max(p.min ?? n, n)));
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            }}
          />
        </label>
      );
    case "select":
      return (
        <label className="field">
          <span className="field-label">{t(p.label)}</span>
          <select className="select" value={mixed ? "" : String(value)} onChange={(e) => onChange(e.target.value)}>
            {mixed && (
              <option value="" disabled>
                {t("Mixed")}
              </option>
            )}
            {p.options.map((o) => (
              <option key={o.value} value={o.value}>
                {t(o.label)}
              </option>
            ))}
          </select>
        </label>
      );
    case "color":
      return (
        <div className="field">
          <span className="field-label">{t(p.label)}</span>
          <div className="swatch-row" role="radiogroup" aria-label={t(p.label)}>
            <button type="button" role="radio" aria-checked={!mixed && !value} className={`swatch-btn is-auto ${!mixed && !value ? "is-on" : ""}`} title={t("Theme")} onClick={() => onChange("")}>
              A
            </button>
            {palette().map((c) => (
              <button key={c} type="button" role="radio" aria-checked={value === c} className={`swatch-btn ${value === c ? "is-on" : ""}`} style={{ background: c }} title={c} onClick={() => onChange(c)} />
            ))}
          </div>
        </div>
      );
  }
}

function FormatPane({ page, visuals }: { page: Page; visuals: Visual[] }) {
  const { t } = useT();
  const edit = useApp((s) => s.edit);
  const objects = commonObjects(visuals.map((v) => v.visualType));
  // The first section starts open (a visual's title, a slicer's settings).
  const [open, setOpen] = useState<Set<string>>(new Set([objects[0]?.name ?? "title"]));
  const ids = visuals.map((v) => v.id);
  // One undo step for every selected visual.
  const setProp = (object: string, name: string, value: unknown) =>
    edit(ids.length > 1 ? "Format visuals" : "Format visual", (m) => {
      const p = m.report.pages.find((x) => x.id === page.id);
      return p && withSync(m, p.visuals.filter((v) => ids.includes(v.id)).map((v) => updateVisual(p.id, withProp(v, object, name, value))));
    });
  if (!objects.length) return <p className="pane-hint">{t("These visuals have no formatting options in common.")}</p>;
  const section = (o: ObjectDef) => {
    const isOpen = open.has(o.name);
    const toggle = o.toggle ? shownValue(visuals, o.name, o.toggle) : undefined;
    return (
      <div key={o.name} className="format-section">
        <div className="format-head">
          <button
            type="button"
            className="format-title"
            aria-expanded={isOpen}
            onClick={() =>
              setOpen((s) => {
                const n = new Set(s);
                if (n.has(o.name)) n.delete(o.name);
                else n.add(o.name);
                return n;
              })
            }
          >
            <ChevronDown size={13} className={isOpen ? "" : "is-closed"} />
            {t(o.label)}
          </button>
          {o.toggle && (
            <label className="switch" title={toggle === MIXED ? t("Mixed") : toggle ? t("On") : t("Off")}>
              <input type="checkbox" checked={toggle === true} ref={(el) => void (el && (el.indeterminate = toggle === MIXED))} aria-label={t("Show %1", t(o.label))} onChange={(e) => setProp(o.name, o.toggle!, e.target.checked)} />
              <span />
            </label>
          )}
        </div>
        {isOpen && o.editor && (visuals.length === 1 ? <FormatEditor editor={o.editor} page={page} visual={visuals[0]!} /> : <p className="pane-hint">{t("Select a single visual to edit these.")}</p>)}
        {isOpen && !o.editor && (
          <div className="format-body">
            {o.props
              .filter((p) => p.name !== o.toggle)
              .map((p) => (
                <Control key={p.name} p={p} value={shownValue(visuals, o.name, p.name)} onChange={(v) => setProp(o.name, p.name, v)} />
              ))}
          </div>
        )}
      </div>
    );
  };
  return (
    <div className="format-pane">
      {visuals.length > 1 && <p className="pane-hint">{t("Formatting %1 visuals: the options they share. Changes apply to all of them.", visuals.length)}</p>}
      {objects.map(section)}
    </div>
  );
}

const PAGE_SIZES = [
  { label: "16:9", width: 1280, height: 720 },
  { label: "4:3", width: 960, height: 720 },
  { label: "Letter", width: 816, height: 1056 },
  { label: "Tooltip", width: 320, height: 240 },
];

function PageFormat({ page }: { page: Page }) {
  const { t } = useT();
  const commit = useApp((s) => s.commit);
  const settings = pageSettings(page);
  const size = PAGE_SIZES.find((s) => s.width === page.width && s.height === page.height);
  return (
    <div className="format-pane">
      <p className="pane-hint">{t("Page settings")}</p>
      <label className="field">
        <span className="field-label">{t("Name")}</span>
        <input
          className="input"
          key={page.name}
          defaultValue={page.name}
          onBlur={(e) => e.target.value.trim() && e.target.value.trim() !== page.name && commit("Rename page", [{ op: "setPage", page: page.id, settings: { ...settings, name: e.target.value.trim() } }])}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          }}
        />
      </label>
      <label className="field">
        <span className="field-label">{t("Canvas size")}</span>
        <select
          className="select"
          value={size?.label ?? "custom"}
          onChange={(e) => {
            const s = PAGE_SIZES.find((x) => x.label === e.target.value);
            if (s) commit("Resize page", [{ op: "setPage", page: page.id, settings: { ...settings, width: s.width, height: s.height } }]);
          }}
        >
          {PAGE_SIZES.map((s) => (
            <option key={s.label} value={s.label}>
              {t(s.label)} ({s.width} × {s.height})
            </option>
          ))}
          {!size && <option value="custom">{`${page.width} × ${page.height}`}</option>}
        </select>
      </label>
      <TooltipPageSettings page={page} />
    </div>
  );
}
