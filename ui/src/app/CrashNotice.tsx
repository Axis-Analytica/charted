// After a crash: what happened, and a GitHub issue to report it (filled in, for the
// user to read and send; charted sends nothing itself).
import type { CrashInfo } from "../bridge/generated/CrashInfo";
import { transport } from "../bridge";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { useApp } from "../state/app";

export function CrashNotice({ crash }: { crash: CrashInfo }) {
  const { t } = useT();
  const dismiss = () => void useApp.getState().rpc({ type: "dismissCrash" });
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && dismiss()}
      title={t("charted closed unexpectedly")}
      width={520}
      footer={
        <>
          <Button variant="ghost" onPress={dismiss}>
            {t("Not now")}
          </Button>
          <Button
            variant="primary"
            onPress={() => {
              void transport.openIssue(crash.issueUrl);
              dismiss();
            }}
          >
            {t("Report on GitHub…")}
          </Button>
        </>
      }
    >
      <p className="dialog-message">{t("It stopped on %1 with: %2", new Date(crash.at * 1000).toLocaleString(), crash.message)}</p>
      <p className="dialog-message">{t("Your work was saved for recovery. The report opens as a GitHub issue you can read and edit before sending; charted never sends anything by itself.")}</p>
      <p className="crash-path">{crash.path}</p>
    </Dialog>
  );
}
