// The Layout menu: switch between layout profiles (Standard, Report + model, Two report
// pages and the user's own), put a second view beside the first, undo layout changes,
// and bring back hidden toolbar buttons. Layouts live in the core (app/layout.ts).
import { Check, LayoutPanelLeft } from "lucide-react";
import { useState } from "react";
import type { ViewKind } from "../bridge/generated/ViewKind";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { Menu, type MenuEntry } from "../kit/Menu";
import { transport } from "../bridge";
import { useApp } from "../state/app";
import { toast } from "../state/toasts";
import { HIDEABLE } from "./commands";
import { setLayout, useLayout, useWorkspace, withHidden } from "./layout";
import { useShortcut } from "./shortcuts";

const PROFILE_FILES = [{ name: "charted layout profile", extensions: ["charted-workspace"] }];

const SECOND: { view: ViewKind | null; label: string }[] = [
  { view: null, label: "One view" },
  { view: "report", label: "A second report beside it" },
  { view: "model", label: "The model beside it" },
  { view: "data", label: "The data beside it" },
  { view: "query", label: "Transform data beside it" },
];

export function LayoutMenu() {
  const { t } = useT();
  const ws = useWorkspace();
  const layout = useLayout();
  const state = useApp((s) => s.state);
  const rpc = useApp((s) => s.rpc);
  const [naming, setNaming] = useState<"new" | "rename" | null>(null);
  const [hiddenOpen, setHiddenOpen] = useState(false);
  useShortcut(
    { id: "layout-undo", keys: "", label: "Undo layout change", group: "View", run: () => void rpc({ type: "undoLayout" }), enabled: () => !!useApp.getState().state?.layoutUndo },
    { id: "layout-redo", keys: "", label: "Redo layout change", group: "View", run: () => void rpc({ type: "redoLayout" }), enabled: () => !!useApp.getState().state?.layoutRedo },
    { id: "layout-new", keys: "", label: "New layout profile…", group: "View", run: () => setNaming("new") },
  );
  if (!ws) return null;
  const active = ws.profiles.find((p) => p.id === ws.active) ?? ws.profiles[0]!;
  const exportProfile = async () => {
    const path = await transport.pick({ title: t("Export a layout profile"), saveAs: `${t(active.name)}.charted-workspace`, filters: PROFILE_FILES });
    if (path && (await rpc({ type: "exportProfile", profile: active.id, path }))) toast({ kind: "success", title: t("Exported the layout profile"), detail: path });
  };
  const importProfile = async () => {
    const path = await transport.pick({ title: t("Import a layout profile"), filters: PROFILE_FILES });
    if (path) await rpc({ type: "importProfile", path });
  };
  const items: (MenuEntry | null)[] = [
    ...ws.profiles.map((p) => ({ id: `p:${p.id}`, label: t(p.name), icon: p.id === active.id ? <Check size={14} /> : undefined, run: () => void rpc({ type: "switchProfile", profile: p.id }) })),
    null,
    ...SECOND.map((s) => ({
      id: `s:${s.view ?? "none"}`,
      label: t(s.label),
      icon: (layout.split?.view ?? null) === s.view ? <Check size={14} /> : undefined,
      run: () => void setLayout((l) => ({ ...l, split: s.view ? { view: s.view, ratio: l.split?.ratio ?? 0.5 } : undefined })),
    })),
    null,
    { id: "undo", label: t("Undo layout change"), disabled: !state?.layoutUndo, run: () => void rpc({ type: "undoLayout" }) },
    { id: "redo", label: t("Redo layout change"), disabled: !state?.layoutRedo, run: () => void rpc({ type: "redoLayout" }) },
    { id: "hidden", label: layout.hidden.length ? t("Buttons and menus (%1 hidden)…", layout.hidden.length) : t("Buttons and menus…"), run: () => setHiddenOpen(true) },
    null,
    { id: "new", label: t("New profile…"), run: () => setNaming("new") },
    { id: "rename", label: t("Rename profile…"), run: () => setNaming("rename") },
    ...(active.builtin ? [{ id: "reset", label: t("Reset %1", t(active.name)), run: () => void rpc({ type: "resetProfile", profile: active.id }) }] : []),
    { id: "delete", label: t("Delete %1", t(active.name)), disabled: ws.profiles.length <= 1, run: () => void rpc({ type: "deleteProfile", profile: active.id }) },
    null,
    { id: "export", label: t("Export %1…", t(active.name)), run: () => void exportProfile() },
    { id: "import", label: t("Import a profile…"), run: () => void importProfile() },
  ];
  return (
    <>
      <Menu
        label={t("Layout")}
        trigger={
          <>
            <LayoutPanelLeft size={15} />
            <span className="layout-menu-name">{t(active.name)}</span>
          </>
        }
        items={items}
      />
      {naming && (
        <NameDialog
          title={naming === "new" ? t("New layout profile") : t("Rename %1", t(active.name))}
          initial={naming === "new" ? "" : active.name}
          onCancel={() => setNaming(null)}
          onOk={async (name) => {
            const ok = await rpc(naming === "new" ? { type: "addProfile", name, from: null } : { type: "renameProfile", profile: active.id, name });
            if (ok) setNaming(null);
          }}
        />
      )}
      {hiddenOpen && <HiddenDialog hidden={layout.hidden} onClose={() => setHiddenOpen(false)} />}
    </>
  );
}

function NameDialog({ title, initial, onOk, onCancel }: { title: string; initial: string; onOk(name: string): void; onCancel(): void }) {
  const { t } = useT();
  const [name, setName] = useState(initial);
  return (
    <Dialog
      open
      modal
      onOpenChange={(o) => !o && onCancel()}
      title={title}
      footer={
        <>
          <Button variant="ghost" onPress={onCancel}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={!name.trim()} onPress={() => onOk(name.trim())}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <label className="field">
        <span className="field-label">{t("Name")}</span>
        <input
          className="input"
          aria-label={t("Name")}
          value={name}
          // biome-ignore lint/a11y/noAutofocus: the dialog asks for this one thing
          autoFocus
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && name.trim() && onOk(name.trim())}
        />
      </label>
    </Dialog>
  );
}

/** Every button and menu item that can be hidden, by where it is: untick to hide. */
function HiddenDialog({ hidden, onClose }: { hidden: string[]; onClose(): void }) {
  const { t } = useT();
  const places = [...new Set(HIDEABLE.map((c) => c.place))];
  return (
    <Dialog
      open
      modal
      onOpenChange={(o) => !o && onClose()}
      title={t("Buttons and menus")}
      width={520}
      footer={
        <>
          <Button variant="ghost" isDisabled={!hidden.length} onPress={() => void setLayout((l) => withHidden(l, l.hidden, false))}>
            {t("Show all")}
          </Button>
          <Button variant="primary" onPress={onClose}>
            {t("Done")}
          </Button>
        </>
      }
    >
      <p className="field-hint">{t("Hidden buttons are still in the command palette (Ctrl+K).")}</p>
      <div className="hidden-commands">
        {places.map((place) => (
          <fieldset key={place}>
            <legend>{t(place)}</legend>
            {HIDEABLE.filter((c) => c.place === place).map((c) => (
              <label key={c.id} className="check">
                <input type="checkbox" checked={!hidden.includes(c.id)} onChange={(e) => void setLayout((l) => withHidden(l, [c.id], !e.target.checked))} />
                {t(c.label)}
              </label>
            ))}
          </fieldset>
        ))}
      </div>
    </Dialog>
  );
}
