// A new, empty project: a name and a folder (Documents/charted unless another is
// chosen). It opens on a blank page, ready to get data.
import { FolderOpen } from "lucide-react";
import { useState } from "react";
import { errorMessage, transport } from "../bridge";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";
import { useReport } from "../state/report";
import { create } from "zustand";

/** Whether the New project dialog is open (Ctrl+N, the welcome screen, File). */
export const useNewProject = create<{ open: boolean; set(open: boolean): void }>((set) => ({ open: false, set: (open) => set({ open }) }));

export function NewProjectDialog() {
  const { t } = useT();
  const open = useNewProject((s) => s.open);
  const defaultDir = useApp((s) => s.state?.projectsDir ?? "");
  const [name, setName] = useState("");
  const [dir, setDir] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (!open) return null;
  const close = () => {
    useNewProject.getState().set(false);
    setName("");
    setDir(null);
    setError(null);
  };
  const folder = dir ?? defaultDir;
  const create = async () => {
    if (!name.trim()) return setError(t("Name the project."));
    setBusy(true);
    setError(null);
    try {
      const state = await transport.rpc({ type: "newProject", name: name.trim(), dir });
      useApp.setState({ state });
      useReport.getState().set({ view: "report", page: null, selected: [] });
      close();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && close()}
      title={t("New project")}
      width={500}
      footer={
        <>
          <Button variant="ghost" onPress={close}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" onPress={create} isDisabled={busy || !name.trim()}>
            {t("Create")}
          </Button>
        </>
      }
    >
      <form
        className="new-project"
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <label className="field">
          <span className="field-label">{t("Name")}</span>
          {/* biome-ignore lint/a11y/noAutofocus: the dialog opens to type a name */}
          <input className="input" value={name} autoFocus placeholder={t("e.g. Sales 2026")} onChange={(e) => setName(e.target.value)} aria-label={t("Project name")} />
        </label>
        <div className="field">
          <span className="field-label">{t("Folder")}</span>
          <div className="new-project-folder">
            <span className="new-project-path" title={folder}>
              {folder}/{name.trim() || "…"}
            </span>
            <Button
              size="sm"
              variant="ghost"
              icon={<FolderOpen size={14} />}
              onPress={async () => {
                const picked = await transport.pick({ title: t("Choose where to create the project"), directory: true, create: true });
                if (picked) setDir(picked);
              }}
            >
              {t("Change…")}
            </Button>
          </div>
        </div>
        <p className="new-project-hint">{t("The project is a folder you can keep in git. Next, get data: a CSV, Excel or Parquet file, a folder of files, or a database.")}</p>
        {error && <p className="new-project-error">{error}</p>}
      </form>
    </Dialog>
  );
}
