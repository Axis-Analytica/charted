import { Palette } from "lucide-react";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import type { Table } from "../bridge/generated/Table";
import { useT } from "../i18n";
import { formatNumber } from "../i18n";
import { useApp } from "../state/app";
import { useDataView } from "../state/dataview";
import { currentPage, useReport } from "../state/report";

export function StatusBar({ project, table }: { project: ProjectSnapshot; table: Table | null }) {
  const { t, tn } = useT();
  const env = useApp((s) => s.env);
  const stats = table ? project.tables.find((s) => s.id === table.id) : undefined;
  const counts = useDataView((s) => s.counts);
  const selectedId = useDataView((s) => (table ? s.selected[table.id] : undefined));
  const column = table?.columns.find((c) => c.id === selectedId);
  const live = counts && table && counts.table === table.id ? counts : null;
  const total = Number(stats?.rows ?? 0);
  const view = useReport((s) => s.view);
  const pageId = useReport((s) => s.page);
  const selectedCount = useReport((s) => s.selected.length);
  const page = view === "report" ? currentPage(project.model, pageId) : null;
  const pageIndex = page ? project.model.report.pages.indexOf(page) : -1;
  return (
    <footer className="statusbar">
      <span>{project.dirty ? t("Unsaved changes") : t("All changes saved")}</span>
      {table && stats && (
        <>
          <span className="status-sep" />
          <span data-testid="status-rows">
            {table.name}: {live && live.rows !== null && live.rows !== total ? t("%1 of %2 rows", formatNumber(live.rows), formatNumber(total)) : tn("%n rows", total, "%n row")}
          </span>
          {column && (
            <>
              <span className="status-sep" />
              <span data-testid="status-distinct">
                {column.name}: {live?.distinct != null ? tn("%n distinct values", live.distinct, "%n distinct value") : "…"}
              </span>
            </>
          )}
        </>
      )}
      {page && (
        <>
          <span className="status-sep" />
          <span data-testid="status-page">
            {t("Page %1 of %2", pageIndex + 1, project.model.report.pages.length)} · {tn("%n visuals", page.visuals.length, "%n visual")}
            {selectedCount > 0 && <> · {tn("%n selected", selectedCount, "%n selected")}</>}
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
