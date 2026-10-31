// The theme editor (Themes → Customize current theme…): name, data colours, page
// colours, text classes and a few common visual styles, written as Power BI theme JSON
// (everything else in the theme is kept). Changes show on the canvas as you make them
// (a draft theme in view state); OK commits them as one undoable step, Cancel drops
// them. The theme can be exported as a Power BI theme file.
import { FontPicker } from "./FontPicker";
import { ArrowDown, ArrowUp, Download, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { errorMessage, transport } from "../bridge";
import type { Model } from "../bridge/generated/Model";
import type { ReportTheme } from "../bridge/generated/ReportTheme";
import { msg, useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { useThemeDraft } from "../state/themedraft";
import { toast } from "../state/toasts";
import { dataColors } from "./themes";

export const useThemeEditor = create<{ open: boolean; set(open: boolean): void }>((set) => ({ open: false, set: (open) => set({ open }) }));

type Json = Record<string, unknown>;

/** The text classes the editor offers (Power BI's names). */
const TEXT_CLASSES = [
  { name: "title", label: msg("Visual titles") },
  { name: "label", label: msg("Labels") },
  { name: "callout", label: msg("Card values") },
] as const;

const hex = (v: unknown, fallback: string) => (typeof v === "string" && /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : typeof v === "string" && /^#[0-9a-f]{3}$/i.test(v) ? `#${v.slice(1).replace(/./g, (c) => c + c)}`.toLowerCase() : fallback);

/** The page's current colours when the report follows the desktop. */
function desktopTheme(name: string): ReportTheme {
  const page = document.querySelector(".page") ?? document.documentElement;
  const s = getComputedStyle(page);
  const v = (k: string) => s.getPropertyValue(k).trim();
  const n = Number(v("--chart-count")) || 8;
  const colors = Array.from({ length: n }, (_, i) => v(`--chart-${i + 1}`)).filter((c) => /^#[0-9a-f]{3,8}$/i.test(c));
  const json: Json = { name, dataColors: colors.length ? colors : ["#118DFF"] };
  for (const [k, token] of [["background", "--bg"], ["foreground", "--fg"], ["tableAccent", "--accent"]]) if (/^#[0-9a-f]{3,8}$/i.test(v(token!))) json[k!] = v(token!);
  return { name, json };
}

/** `visualStyles["*"]["*"][object][0][prop]`. */
function styleOf(json: Json, object: string, prop: string): unknown {
  const vs = json.visualStyles as Record<string, Record<string, Record<string, Json[]>>> | undefined;
  return vs?.["*"]?.["*"]?.[object]?.[0]?.[prop];
}

/** Sets (or with `undefined` removes) a wildcard visual style, keeping everything else. */
export function withStyle(json: Json, object: string, prop: string, value: unknown): Json {
  const vs = { ...((json.visualStyles as Json | undefined) ?? {}) } as Record<string, Record<string, Record<string, Json[]>>>;
  const all = { ...(vs["*"] ?? {}) };
  const star = { ...(all["*"] ?? {}) };
  const first = { ...(star[object]?.[0] ?? {}) };
  if (value === undefined) delete first[prop];
  else first[prop] = value;
  if (Object.keys(first).length) star[object] = [first, ...(star[object]?.slice(1) ?? [])];
  else delete star[object];
  all["*"] = star;
  vs["*"] = all;
  return { ...json, visualStyles: vs };
}

/** Sets (or removes) a text class property. */
export function withTextClass(json: Json, cls: string, prop: string, value: unknown): Json {
  const tc = { ...((json.textClasses as Record<string, Json> | undefined) ?? {}) };
  const c = { ...(tc[cls] ?? {}) };
  if (value === undefined || value === "") delete c[prop];
  else c[prop] = value;
  if (Object.keys(c).length) tc[cls] = c;
  else delete tc[cls];
  return { ...json, textClasses: tc };
}

export function ThemeEditor({ model }: { model: Model }) {
  const { t } = useT();
  const open = useThemeEditor((s) => s.open);
  const setOpen = useThemeEditor((s) => s.set);
  const commit = useApp((s) => s.commit);
  const draft = useThemeDraft((s) => s.draft);
  const setDraft = useThemeDraft((s) => s.set);
  const [start, setStart] = useState<ReportTheme | null>(null);

  // Opening starts from the current theme (or the desktop's colours).
  useEffect(() => {
    if (!open) return;
    const current = model.report.theme ?? desktopTheme(t("Custom theme"));
    setStart(model.report.theme ?? null);
    setDraft({ name: current.name, json: { ...(current.json as Json), name: current.name } });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const theme = draft ?? null;
  const json = (theme?.json ?? {}) as Json;
  const update = (next: Json, name = theme?.name ?? "") => setDraft({ name, json: { ...next, name } });
  const colors = theme ? dataColors(theme) : [];
  const setColors = (c: string[]) => update({ ...json, dataColors: c });
  const close = () => {
    setOpen(false);
    setDraft(undefined);
  };
  const ok = async () => {
    if (theme && JSON.stringify(theme) !== JSON.stringify(start)) await commit("Change theme", [{ op: "setTheme", theme }]);
    close();
  };
  const exportTheme = async () => {
    if (!theme) return;
    const path = await transport.pick({ title: t("Export theme"), saveAs: `${theme.name || "theme"}.json`, filters: [{ name: t("Power BI theme"), extensions: ["json"] }] });
    if (!path) return;
    try {
      await transport.rpc({ type: "exportTheme", path, theme });
      toast({ kind: "success", title: t("Exported the theme"), detail: path });
    } catch (e) {
      toast({ kind: "error", title: errorMessage(e) });
    }
  };

  const tri = (v: unknown) => (v === true ? "on" : v === false ? "off" : "");
  const fromTri = (v: string) => (v === "on" ? true : v === "off" ? false : undefined);
  const textClasses = (json.textClasses ?? {}) as Record<string, Json>;

  return (
    <Dialog
      open={open && !!theme}
      onOpenChange={(o) => !o && close()}
      title={t("Customize theme")}
      width={520}
      modal
      footer={
        <>
          <Button variant="ghost" icon={<Download size={14} />} onPress={exportTheme}>
            {t("Export theme…")}
          </Button>
          <span className="dialog-spacer" />
          <Button variant="ghost" onPress={close}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={!theme?.name.trim() || !colors.length} onPress={ok}>
            {t("OK")}
          </Button>
        </>
      }
    >
      {theme && (
        <div className="theme-editor">
          <label className="field">
            <span className="field-label">{t("Theme name")}</span>
            <input className="input" value={theme.name} onChange={(e) => update(json, e.target.value)} />
          </label>

          <fieldset className="theme-section">
            <legend>{t("Data colors")}</legend>
            <ol className="theme-colors">
              {colors.map((c, i) => (
                <li key={i}>
                  <input
                    type="color"
                    aria-label={t("Color %1", i + 1)}
                    value={hex(c, "#118dff")}
                    onChange={(e) => setColors(colors.map((x, j) => (j === i ? e.target.value : x)))}
                  />
                  <span className="mono">{c}</span>
                  <span className="status-fill" />
                  <IconButton label={t("Move up")} icon={<ArrowUp size={13} />} isDisabled={i === 0} onPress={() => setColors(colors.map((x, j) => (j === i - 1 ? c : j === i ? colors[i - 1]! : x)))} />
                  <IconButton label={t("Move down")} icon={<ArrowDown size={13} />} isDisabled={i === colors.length - 1} onPress={() => setColors(colors.map((x, j) => (j === i + 1 ? c : j === i ? colors[i + 1]! : x)))} />
                  <IconButton label={t("Remove color %1", i + 1)} icon={<X size={13} />} isDisabled={colors.length < 2} onPress={() => setColors(colors.filter((_, j) => j !== i))} />
                </li>
              ))}
            </ol>
            <Button size="sm" variant="ghost" icon={<Plus size={14} />} isDisabled={colors.length >= 20} onPress={() => setColors([...colors, colors[colors.length % Math.max(1, colors.length)] ?? "#118dff"])}>
              {t("Add color")}
            </Button>
          </fieldset>

          <fieldset className="theme-section theme-grid">
            <legend>{t("Page")}</legend>
            {[
              { key: "background", label: msg("Background"), fallback: "#ffffff" },
              { key: "foreground", label: msg("Foreground"), fallback: "#252423" },
              { key: "tableAccent", label: msg("Table accent"), fallback: colors[0] ?? "#118dff" },
            ].map((x) => (
              <label key={x.key} className="theme-color-field">
                <input type="color" value={hex(json[x.key], x.fallback)} onChange={(e) => update({ ...json, [x.key]: e.target.value })} />
                <span>{t(x.label)}</span>
              </label>
            ))}
          </fieldset>

          <fieldset className="theme-section">
            <legend>{t("Text")}</legend>
            <div className="theme-text">
              {TEXT_CLASSES.map((c) => {
                const tc = textClasses[c.name] ?? {};
                return (
                  <div key={c.name} className="theme-text-row" role="group" aria-label={t(c.label)}>
                    <span className="theme-text-name">{t(c.label)}</span>
                    <FontPicker label={t("Font")} value={typeof tc.fontFace === "string" ? tc.fontFace : ""} onChange={(f) => update(withTextClass(json, c.name, "fontFace", f))} />
                    <input
                      className="input theme-size"
                      type="number"
                      min={6}
                      max={96}
                      aria-label={t("Text size")}
                      placeholder={t("Default")}
                      value={typeof tc.fontSize === "number" ? tc.fontSize : ""}
                      onChange={(e) => update(withTextClass(json, c.name, "fontSize", e.target.value === "" ? undefined : Number(e.target.value)))}
                    />
                    <input type="color" aria-label={t("Text color")} value={hex(tc.color, hex(json.foreground, "#252423"))} onChange={(e) => update(withTextClass(json, c.name, "color", e.target.value))} />
                  </div>
                );
              })}
            </div>
          </fieldset>

          <fieldset className="theme-section theme-grid">
            <legend>{t("Visuals")}</legend>
            <label className="field">
              <span className="field-label">{t("Titles")}</span>
              <select className="select" value={tri(styleOf(json, "title", "show"))} onChange={(e) => update(withStyle(json, "title", "show", fromTri(e.target.value)))}>
                <option value="">{t("Default")}</option>
                <option value="on">{t("On")}</option>
                <option value="off">{t("Off")}</option>
              </select>
            </label>
            <label className="field">
              <span className="field-label">{t("Title size")}</span>
              <input
                className="input"
                type="number"
                min={8}
                max={40}
                placeholder={t("Default")}
                value={typeof styleOf(json, "title", "fontSize") === "number" ? (styleOf(json, "title", "fontSize") as number) : ""}
                onChange={(e) => update(withStyle(json, "title", "fontSize", e.target.value === "" ? undefined : Number(e.target.value)))}
              />
            </label>
            <label className="field">
              <span className="field-label">{t("Legend position")}</span>
              <select className="select" value={typeof styleOf(json, "legend", "position") === "string" ? (styleOf(json, "legend", "position") as string) : ""} onChange={(e) => update(withStyle(json, "legend", "position", e.target.value || undefined))}>
                <option value="">{t("Default")}</option>
                <option value="Top">{t("Top")}</option>
                <option value="Bottom">{t("Bottom")}</option>
                <option value="Right">{t("Right")}</option>
              </select>
            </label>
            <label className="field">
              <span className="field-label">{t("Data labels")}</span>
              <select className="select" value={tri(styleOf(json, "labels", "show"))} onChange={(e) => update(withStyle(json, "labels", "show", fromTri(e.target.value)))}>
                <option value="">{t("Default")}</option>
                <option value="on">{t("On")}</option>
                <option value="off">{t("Off")}</option>
              </select>
            </label>
          </fieldset>
        </div>
      )}
    </Dialog>
  );
}
