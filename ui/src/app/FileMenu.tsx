import { FileDown, FileUp, FolderOutput, Save, X } from "lucide-react";
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
import { useShortcut } from "./shortcuts";

const OSSIE = [{ name: "Apache Ossie", extensions: ["yaml", "yml"] }];

/** File commands: save, export (Power BI project, Apache Ossie), import, close. */
export function FileMenu({ project, onSave, onClose }: { project: ProjectSnapshot; onSave(): void; onClose(): void }) {
  const { t } = useT();
  const rpc = useApp((s) => s.rpc);
  const [pending, setPending] = useState<{ path: string; diff: ModelDiff } | null>(null);

  const exportPbip = async () => {
    const dir = await transport.pick({ title: t("Export to a folder for Power BI"), directory: true, create: true });
    if (!dir) return;
    if (await rpc({ type: "exportPbip", dir }, t("Exporting…"))) {
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
  );

  const diff = pending?.diff;
  return (
    <>
      <Menu
        label={t("File")}
        trigger={t("File")}
        items={[
          { id: "save", label: t("Save"), icon: <Save size={14} />, shortcut: "Ctrl+S", disabled: !project.dirty, run: onSave },
          null,
          { id: "export-pbip", label: t("Export Power BI project…"), icon: <FolderOutput size={14} />, run: exportPbip },
          { id: "export-ossie", label: t("Export Apache Ossie model…"), icon: <FileDown size={14} />, run: exportOssie },
          { id: "import-ossie", label: t("Import Apache Ossie model…"), icon: <FileUp size={14} />, run: importOssie },
          null,
          { id: "close", label: t("Close project"), icon: <X size={14} />, shortcut: "Ctrl+W", run: onClose },
        ]}
      />
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
