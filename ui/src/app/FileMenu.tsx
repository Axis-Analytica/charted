import { FilePlus2, FileArchive, FileDown, FileInput, FileText, FileUp, FolderOutput, Save, X } from "lucide-react";
import { useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { ModelDiff } from "../bridge/generated/ModelDiff";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { Menu } from "../kit/Menu";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";
import { toast } from "../state/toasts";
import { useNewProject } from "./NewProject";
import { useShortcut } from "./shortcuts";
import { exportPdf } from "../report/PrintLayout";

const OSSIE = [{ name: "Apache Ossie", extensions: ["yaml", "yml"] }];
export const POWER_BI = [{ name: "Power BI project", extensions: ["pbip", "bim"] }];
const BUNDLE = [{ name: "charted bundle", extensions: ["chartedz"] }];

/** Imports a Power BI project as a new charted project (shown with its import report). */
export async function importPowerBi(t: (s: string) => string) {
  const path = await transport.pick({ title: t("Import a Power BI project"), filters: POWER_BI });
  if (path) await useApp.getState().rpc({ type: "importPowerBi", path, dir: null }, t("Importing…"));
}

/** File commands: save, export (Power BI project, Apache Ossie), import, close. */
export function FileMenu({ project, onSave, onClose }: { project: ProjectSnapshot; onSave(): void; onClose(): void }) {
  const { t } = useT();
  const rpc = useApp((s) => s.rpc);
  const [pending, setPending] = useState<{ path: string; diff: ModelDiff } | null>(null);
  const [exporting, setExporting] = useState<{ dir: string; withData: boolean; folder: string } | null>(null);

  const exportPbip = async () => {
    const dir = await transport.pick({ title: t("Export to a folder for Power BI"), directory: true, create: true });
    if (dir) setExporting({ dir, withData: true, folder: "" });
  };
  const runExport = async () => {
    if (!exporting) return;
    const { dir, withData, folder } = exporting;
    setExporting(null);
    if (await rpc({ type: "exportPbip", dir, options: { withData, sourceFolder: folder.trim() || null } }, t("Exporting…"))) {
      toast({ kind: "success", title: t("Exported a Power BI project"), detail: `${dir}/${project.name}.pbip` });
    }
  };
  const exportOssie = async () => {
    const path = await transport.pick({ title: t("Export the model as Apache Ossie"), saveAs: `${project.name}.ossie.yaml`, filters: OSSIE });
    if (!path) return;
    if (await rpc({ type: "exportOssie", path })) toast({ kind: "success", title: t("Exported the model"), detail: path });
  };
  const importOssie = async () => {
    const path = await transport.pick({ title: t("Import an Apache Ossie model"), filters: OSSIE });
    if (!path) return;
    try {
      setPending({ path, diff: await dataJson<ModelDiff>({ type: "ossieDiff", path }) });
    } catch (e) {
      toast({ kind: "error", title: errorMessage(e) });
    }
  };
  const saveBundle = async () => {
    const path = await transport.pick({ title: t("Save as a bundle to share"), saveAs: `${project.name}.chartedz`, filters: BUNDLE });
    if (!path) return;
    if (await rpc({ type: "saveBundle", path }, t("Saving…"))) toast({ kind: "success", title: t("Saved a bundle"), detail: path });
  };
  const apply = async () => {
    if (!pending) return;
    const { path } = pending;
    setPending(null);
    if (await rpc({ type: "importOssie", path })) toast({ kind: "success", title: t("Imported the model") });
  };

  useShortcut(
    { id: "export-pbip", keys: "", label: "Export Power BI project…", group: "File", run: exportPbip },
    { id: "export-ossie", keys: "", label: "Export Apache Ossie model…", group: "File", run: exportOssie },
    { id: "import-ossie", keys: "", label: "Import Apache Ossie model…", group: "File", run: importOssie },
    { id: "import-pbip", keys: "", label: "Import Power BI project…", group: "File", run: () => importPowerBi(t) },
    { id: "save-bundle", keys: "", label: "Save as bundle…", group: "File", run: saveBundle },
    { id: "export-pdf", keys: "", label: "Export to PDF…", group: "File", run: () => void exportPdf(t) },
  );

  const diff = pending?.diff;
  return (
    <>
      <Menu
        label={t("File")}
        trigger={t("File")}
        items={[
          { id: "new", label: t("New project…"), icon: <FilePlus2 size={14} />, shortcut: "Ctrl+N", run: () => useNewProject.getState().set(true) },
          { id: "save", label: t("Save"), icon: <Save size={14} />, shortcut: "Ctrl+S", disabled: !project.dirty, run: onSave },
          { id: "save-bundle", label: t("Save as bundle…"), icon: <FileArchive size={14} />, run: saveBundle },
          null,
          { id: "import-pbip", label: t("Import Power BI project…"), icon: <FileInput size={14} />, run: () => importPowerBi(t) },
          { id: "export-pbip", label: t("Export Power BI project…"), icon: <FolderOutput size={14} />, run: exportPbip },
          { id: "export-ossie", label: t("Export Apache Ossie model…"), icon: <FileDown size={14} />, run: exportOssie },
          { id: "export-pdf", label: t("Export to PDF…"), icon: <FileText size={14} />, run: () => void exportPdf(t) },
          { id: "import-ossie", label: t("Import Apache Ossie model…"), icon: <FileUp size={14} />, run: importOssie },
          null,
          { id: "close", label: t("Close project"), icon: <X size={14} />, shortcut: "Ctrl+W", run: onClose },
        ]}
      />
      <Dialog
        open={!!exporting}
        onOpenChange={(o) => !o && setExporting(null)}
        title={t("Export Power BI project")}
        width={520}
        footer={
          <>
            <Button variant="ghost" onPress={() => setExporting(null)}>
              {t("Cancel")}
            </Button>
            <Button variant="primary" onPress={runExport}>
              {t("Export")}
            </Button>
          </>
        }
      >
        {exporting && (
          <div className="export-pbip">
            <p className="dialog-message mono">{exporting.dir}</p>
            <label className="check-row">
              <input type="checkbox" checked={exporting.withData} onChange={(e) => setExporting({ ...exporting, withData: e.target.checked })} />
              <span>{t("Copy the data files into the export")}</span>
            </label>
            <label className="field">
              <span className="field-label">{t("Folder Power BI reads the data from")}</span>
              <input
                className="input"
                value={exporting.folder}
                placeholder={exporting.withData ? exporting.dir : project.path.slice(0, project.path.lastIndexOf("/"))}
                aria-label={t("Folder Power BI reads the data from")}
                onChange={(e) => setExporting({ ...exporting, folder: e.target.value })}
              />
              <span className="field-hint">{t("Opening it on Windows? Type where the folder will be there, such as C:\\Users\\you\\Documents\\Sales\\. Power BI calls it the SourceFolder parameter, and you can change it later in Transform data.")}</span>
            </label>
          </div>
        )}
      </Dialog>
      <Dialog
        open={!!pending}
        onOpenChange={(o) => !o && setPending(null)}
        title={t("Import Apache Ossie model")}
        width={560}
        footer={
          <>
            <Button variant="ghost" onPress={() => setPending(null)}>
              {t("Cancel")}
            </Button>
            <Button variant="primary" isDisabled={!diff?.differences.length} onPress={apply}>
              {t("Import")}
            </Button>
          </>
        }
      >
        {diff && (
          <div className="import-diff">
            <p className="dialog-message">
              {diff.differences.length === 0 ? t("0 differences: this document matches the model.") : t("%1 differences. Importing replaces the model; the report is kept, and Undo reverts it.", diff.differences.length)}
            </p>
            {diff.differences.length > 0 && (
              <ul className="import-diff-list">
                {diff.differences.map((d) => (
                  <li key={d}>{d}</li>
                ))}
              </ul>
            )}
            {diff.warnings.map((w) => (
              <p key={w} className="import-diff-warning">
                {w}
              </p>
            ))}
          </div>
        )}
      </Dialog>
    </>
  );
}
