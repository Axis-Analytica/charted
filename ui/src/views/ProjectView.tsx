import { ChartColumnBig, Command, Keyboard, Redo2, Save, Sheet, Undo2, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Banner } from "../kit/misc";
import { useApp } from "../state/app";
import { toast } from "../state/toasts";
import { useShortcut } from "../app/shortcuts";
import { StatusBar } from "./StatusBar";
import { DataView } from "./data/DataView";
import { useDataView } from "../state/dataview";
import { TablesPanel } from "./TablesPanel";

export function ProjectView({ project }: { project: ProjectSnapshot }) {
  const { t } = useT();
  const rpc = useApp((s) => s.rpc);
  const [selected, setSelected] = useState<string | null>(project.model.tables[0]?.id ?? null);
  useEffect(() => {
    if (selected && !project.model.tables.some((x) => x.id === selected)) setSelected(project.model.tables[0]?.id ?? null);
  }, [project, selected]);
  // Sort, filters and selection belong to the project they were made in.
  useEffect(() => useDataView.getState().reset(), [project.path]);

  const save = async () => {
    if (await rpc({ type: "save" })) toast({ kind: "success", title: t("Saved %1", project.name) });
  };
  const undo = () => rpc({ type: "undo" });
  const redo = () => rpc({ type: "redo" });
  useShortcut(
    { id: "save", keys: "Ctrl+S", label: "Save", group: "File", run: save, global: true },
    { id: "undo", keys: "Ctrl+Z", label: "Undo", group: "Edit", run: undo, enabled: () => !!useApp.getState().state?.project?.undoLabel },
    { id: "redo", keys: "Ctrl+Shift+Z", label: "Redo", group: "Edit", run: redo, enabled: () => !!useApp.getState().state?.project?.redoLabel },
    { id: "data-view", keys: "Ctrl+2", label: "Data view", group: "View", run: () => {} },
    { id: "close", keys: "Ctrl+W", label: "Close project", group: "File", run: () => rpc({ type: "closeProject" }), enabled: () => !useApp.getState().state?.project?.dirty },
  );

  const table = project.model.tables.find((x) => x.id === selected) ?? null;
  return (
    <div className="shell">
      <header className="topbar">
        <div className="topbar-title">
          <ChartColumnBig size={18} className="topbar-mark" />
          <span className="project-name">{project.name}</span>
          {project.dirty && <span className="dirty-dot" title={t("Unsaved changes")} aria-label={t("Unsaved changes")} />}
        </div>
        <div className="topbar-actions">
          <IconButton label={project.undoLabel ? t("Undo %1", t(project.undoLabel)) : t("Undo")} shortcut="Ctrl+Z" icon={<Undo2 size={16} />} isDisabled={!project.undoLabel} onPress={undo} />
          <IconButton label={project.redoLabel ? t("Redo %1", t(project.redoLabel)) : t("Redo")} shortcut="Ctrl+Shift+Z" icon={<Redo2 size={16} />} isDisabled={!project.redoLabel} onPress={redo} />
          <IconButton label={t("Save")} shortcut="Ctrl+S" icon={<Save size={16} />} isDisabled={!project.dirty} onPress={save} />
          <span className="topbar-sep" />
          <IconButton label={t("Command palette")} shortcut="Ctrl+K" icon={<Command size={16} />} onPress={() => window.dispatchEvent(new Event("charted:palette"))} />
          <IconButton label={t("Keyboard shortcuts")} shortcut="?" icon={<Keyboard size={16} />} onPress={() => window.dispatchEvent(new Event("charted:help"))} />
          <IconButton label={t("Close project")} shortcut="Ctrl+W" icon={<X size={16} />} onPress={() => (project.dirty ? toast({ kind: "info", title: t("Save or undo your changes before closing") }) : rpc({ type: "closeProject" }))} />
        </div>
      </header>
      {project.recovery && (
        <Banner
          tone="warning"
          actions={
            <>
              <Button size="sm" variant="ghost" onPress={() => rpc({ type: "discardRecovery" })}>
                {t("Discard")}
              </Button>
              <Button size="sm" variant="primary" onPress={() => rpc({ type: "restoreRecovery" })}>
                {t("Restore")}
              </Button>
            </>
          }
        >
          {t("charted found unsaved changes from %1. Restore them?", new Date(project.recovery.savedAt * 1000).toLocaleString())}
        </Banner>
      )}
      {project.warnings.length > 0 && <Banner tone="info">{t("Some parts of the model were imported with warnings: %1", project.warnings.join("; "))}</Banner>}
      <div className="workspace">
        <nav className="rail" aria-label={t("Views")}>
          <IconButton label={t("Data view")} shortcut="Ctrl+2" icon={<Sheet size={18} />} pressed />
        </nav>
        <TablesPanel
          project={project}
          selected={selected}
          onSelect={setSelected}
          onSelectColumn={(tableId, column) => {
            setSelected(tableId);
            useDataView.getState().select(tableId, column);
          }}
        />
        <main className="main">{table ? <DataView key={`${project.path}\n${table.id}`} project={project} table={table} /> : <div className="main-empty">{t("This project has no tables yet.")}</div>}</main>
      </div>
      <StatusBar project={project} table={table} />
    </div>
  );
}
