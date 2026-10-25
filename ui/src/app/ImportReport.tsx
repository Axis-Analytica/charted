// What a Power BI import brought across and what it didn't, shown once after the
// import (the core keeps it until it's dismissed).
import { AlertTriangle, CheckCircle2, Info } from "lucide-react";
import type { ImportReport } from "../bridge/generated/ImportReport";
import { msg, useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";

// The kinds of things an import can skip, in the order they're listed (also their labels).
const KIND_ORDER = [msg("data"), msg("query"), msg("table"), msg("measure"), msg("column"), msg("relationship"), msg("hierarchy"), msg("visual"), msg("field"), msg("role"), msg("perspective"), msg("culture")];

export function ImportReportDialog({ report }: { report: ImportReport }) {
  const { t } = useT();
  const rpc = useApp((s) => s.rpc);
  const close = () => void rpc({ type: "dismissImportReport" });
  const skipped = [...report.skipped].sort((a, b) => (KIND_ORDER.indexOf(a.kind) + 100) % 100 - (KIND_ORDER.indexOf(b.kind) + 100) % 100);
  const name = report.source.split(/[\\/]/).pop() ?? report.source;
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && close()}
      title={t("Imported %1", name)}
      width={640}
      footer={
        <Button variant="primary" onPress={close}>
          {t("Done")}
        </Button>
      }
    >
      <div className="import-report">
        <p className="import-report-summary">
          <CheckCircle2 size={16} />
          {t("%1 tables (%2 with data), %3 measures, %4 relationships, %5 pages with %6 visuals.", report.tables, report.loaded, report.measures, report.relationships, report.pages, report.visuals)}
        </p>
        {report.notes.map((n) => (
          <p key={n} className="import-report-note">
            <Info size={14} />
            {n}
          </p>
        ))}
        {skipped.length > 0 ? (
          <>
            <h3 className="import-report-head">
              <AlertTriangle size={14} />
              {t("Not imported, or only partly (%1)", skipped.length)}
            </h3>
            <table className="import-report-table">
              <tbody>
                {skipped.map((s, i) => (
                  <tr key={i}>
                    <td className="import-report-kind">{t(s.kind)}</td>
                    <td className="import-report-name">{s.name}</td>
                    <td>{s.reason}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        ) : (
          <p className="import-report-note">{t("Everything came across.")}</p>
        )}
      </div>
    </Dialog>
  );
}
