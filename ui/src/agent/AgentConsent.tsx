// Asked once per agent before its first request: what its provider will see.
import { ShieldCheck } from "lucide-react";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";

export function AgentConsent({ agent, data, onAgree, onCancel }: { agent: string; data: boolean; onAgree(): void; onCancel(): void }) {
  const { t } = useT();
  return (
    <Dialog
      open
      modal
      onOpenChange={(o) => !o && onCancel()}
      title={t("Send requests to %1?", agent)}
      width={480}
      footer={
        <>
          <Button variant="ghost" onPress={onCancel}>
            {t("Not now")}
          </Button>
          <Button variant="primary" icon={<ShieldCheck size={15} />} onPress={onAgree}>
            {t("Agree and send")}
          </Button>
        </>
      }
    >
      <div className="agent-consent">
        <p>{t("%1 runs on this computer, but it sends your requests to its provider's servers, along with what it reads from charted:", agent)}</p>
        <ul>
          <li>{t("the names and types of your tables, columns and measures, and your DAX;")}</li>
          <li>{t("the pages and visuals of your report;")}</li>
          <li>{t("the totals the visuals it checks show, with their category labels (up to 50 rows).")}</li>
        </ul>
        <p>{data ? t("Data access is on for this project: it can also read rows and lists of values.") : t("Rows and lists of values stay here unless you turn on Let the agent see data.")}</p>
        <p className="field-hint">{t("It can't read your files or run commands: charted gives it only its own tools. It uses your agent's own account and plan.")}</p>
      </div>
    </Dialog>
  );
}
