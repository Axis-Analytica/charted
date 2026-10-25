import { ChartColumnBig, ChartPie, Command, Keyboard, Network, Redo2, Save, Sheet, Undo2, Workflow, X } from "lucide-react";
import { lazy, Suspense, useEffect, useState } from "react";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Banner, Spinner } from "../kit/misc";
import { useApp } from "../state/app";
import { toast } from "../state/toasts";
import { useShortcut } from "../app/shortcuts";
import { cycleRegion } from "../app/regions";
import { preloadDaxEditor } from "../dax/LazyDaxEditor";
import { StatusBar } from "./StatusBar";
// The data, model and query views load on first use, keeping the initial download small.
const DataView = lazy(() => import("./data/DataView").then((m) => ({ default: m.DataView })));
import { useDataView } from "../state/dataview";
import { type View, useReport } from "../state/report";
import { ReportView } from "../report/ReportView";
import { TablesPanel } from "./TablesPanel";
import { FileMenu } from "../app/FileMenu";
import { ImportReportDialog } from "../app/ImportReport";
const ModelView = lazy(() => import("../model/ModelView").then((m) => ({ default: m.ModelView })));
const QueryEditor = lazy(() => import("../query/QueryEditor").then((m) => ({ default: m.QueryEditor })));
import { SourceBanner } from "../query/SourceBanner";

export function ProjectView({ project }: { project: ProjectSnapshot }) {
  const { t } = useT();
  const view = useReport((s) => s.view);
  const setView = (v: View) => useReport.getState().set({ view: v });
  const rpc = useApp((s) => s.rpc);
  const [selected, setSelected] = useState<string | null>(project.model.tables[0]?.id ?? null);
  useEffect(() => {
    if (selected && !project.model.tables.some((x) => x.id === selected)) setSelected(project.model.tables[0]?.id ?? null);
  }, [project, selected]);
  // Sort, filters and selection belong to the project they were made in.
  useEffect(() => {
    useDataView.getState().reset();
    useReport.getState().set({ page: null, selected: [], formula: null, field: null, pointer: null });
  }, [project.path]);

  const save = async () => {
    if (await rpc({ type: "save" })) toast({ kind: "success", title: t("Saved %1", project.name) });
  };
  const close = () => (project.dirty ? toast({ kind: "info", title: t("Save or undo your changes before closing") }) : rpc({ type: "closeProject" }));
  const undo = () => rpc({ type: "undo" });
  const redo = () => rpc({ type: "redo" });
  useEffect(preloadDaxEditor, []);
  useShortcut(
    { id: "next-region", keys: "F6", label: "Next region", group: "View", global: true, run: () => cycleRegion(false) },
    { id: "previous-region", keys: "Shift+F6", label: "Previous region", group: "View", global: true, run: () => cycleRegion(true) },
  );
  useShortcut(
    { id: "save", keys: "Ctrl+S", label: "Save", group: "File", run: save, global: true },
    // The query editor has its own undo for its draft.
    { id: "undo", keys: "Ctrl+Z", label: "Undo", group: "Edit", run: undo, enabled: () => !!useApp.getState().state?.project?.undoLabel && useReport.getState().view !== "query" },
    { id: "redo", keys: "Ctrl+Shift+Z", label: "Redo", group: "Edit", run: redo, enabled: () => !!useApp.getState().state?.project?.redoLabel && useReport.getState().view !== "query" },
    { id: "report-view", keys: "Ctrl+1", label: "Report view", group: "View", run: () => setView("report"), global: true },
    { id: "data-view", keys: "Ctrl+2", label: "Data view", group: "View", run: () => setView("data"), global: true },
    { id: "model-view", keys: "Ctrl+3", label: "Model view", group: "View", run: () => setView("model"), global: true },
    { id: "query-view", keys: "Ctrl+4", label: "Transform data", group: "View", run: () => setView("query"), global: true },
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
          <FileMenu project={project} onSave={save} onClose={close} />
        </div>
        <div className="topbar-actions">
          <IconButton label={project.undoLabel ? t("Undo %1", t(project.undoLabel)) : t("Undo")} shortcut="Ctrl+Z" icon={<Undo2 size={16} />} isDisabled={!project.undoLabel} onPress={undo} />
          <IconButton label={project.redoLabel ? t("Redo %1", t(project.redoLabel)) : t("Redo")} shortcut="Ctrl+Shift+Z" icon={<Redo2 size={16} />} isDisabled={!project.redoLabel} onPress={redo} />
          <IconButton label={t("Save")} shortcut="Ctrl+S" icon={<Save size={16} />} isDisabled={!project.dirty} onPress={save} />
          <span className="topbar-sep" />
          <IconButton label={t("Command palette")} shortcut="Ctrl+K" icon={<Command size={16} />} onPress={() => window.dispatchEvent(new Event("charted:palette"))} />
          <IconButton label={t("Keyboard shortcuts")} shortcut="?" icon={<Keyboard size={16} />} onPress={() => window.dispatchEvent(new Event("charted:help"))} />
          <IconButton label={t("Close project")} shortcut="Ctrl+W" icon={<X size={16} />} onPress={close} />
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
      <SourceBanner project={project} />
      {project.importReport && <ImportReportDialog report={project.importReport} />}
      <div className={`workspace ${view === "report" ? "is-report" : ""} ${view === "model" ? "is-model" : ""} ${view === "query" ? "is-query" : ""}`}>
        <nav className="rail" aria-label={t("Views")}>
          <IconButton label={t("Report view")} shortcut="Ctrl+1" icon={<ChartPie size={18} />} pressed={view === "report"} onPress={() => setView("report")} />
          <IconButton label={t("Data view")} shortcut="Ctrl+2" icon={<Sheet size={18} />} pressed={view === "data"} onPress={() => setView("data")} />
          <IconButton label={t("Model view")} shortcut="Ctrl+3" icon={<Network size={18} />} pressed={view === "model"} onPress={() => setView("model")} />
          <IconButton label={t("Transform data")} shortcut="Ctrl+4" icon={<Workflow size={18} />} pressed={view === "query"} onPress={() => setView("query")} />
        </nav>
        {view === "report" ? (
          <main className="main">
            <ReportView model={project.model} />
          </main>
        ) : view === "model" ? (
          <main className="main">
            <Suspense fallback={<ViewLoading />}>
              <ModelView model={project.model} />
            </Suspense>
          </main>
        ) : view === "query" ? (
          <main className="main">
            <Suspense fallback={<ViewLoading />}>
              <QueryEditor project={project} />
            </Suspense>
          </main>
        ) : (
          <>
            <TablesPanel
              project={project}
              selected={selected}
              onSelect={setSelected}
              onSelectColumn={(tableId, column) => {
                setSelected(tableId);
                useDataView.getState().select(tableId, column);
              }}
            />
            <main className="main">
              <Suspense fallback={<ViewLoading />}>
                {table ? <DataView key={`${project.path}\n${table.id}`} project={project} table={table} /> : <div className="main-empty">{t("This project has no tables yet.")}</div>}
              </Suspense>
            </main>
          </>
        )}
      </div>
      <StatusBar project={project} table={view === "data" ? table : null} />
    </div>
  );
}

function ViewLoading() {
  return (
    <div className="main-empty">
      <Spinner size={18} />
    </div>
  );
}
