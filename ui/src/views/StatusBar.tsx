import { Palette } from "lucide-react";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import type { Table } from "../bridge/generated/Table";
import { useT } from "../i18n";
import { useApp } from "../state/app";

export function StatusBar({ project, table }: { project: ProjectSnapshot; table: Table | null }) {
  const { t, tn } = useT();
  const env = useApp((s) => s.env);
  const stats = table ? project.tables.find((s) => s.id === table.id) : undefined;
  return (
    <footer className="statusbar">
      <span>{project.dirty ? t("Unsaved changes") : t("All changes saved")}</span>
      {table && stats && (
        <>
          <span className="status-sep" />
          <span>
            {table.name}: {tn("%n rows", Number(stats.rows), "%n row")} · {tn("%n columns", table.columns.length, "%n column")}
          </span>
        </>
      )}
      <span className="status-fill" />
      {env && (
        <>
          <span title={env.theme.fromOmarchy ? t("Following your Omarchy theme") : t("Not running on Omarchy; using the default theme")}>
            <Palette size={12} /> {env.theme.fromOmarchy ? (env.theme.name ?? t("Omarchy theme")) : t("Default theme")}
          </span>
          <span className="status-sep" />
          <span>{env.language.code}</span>
        </>
      )}
    </footer>
  );
}
