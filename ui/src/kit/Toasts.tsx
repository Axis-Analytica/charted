import { CircleAlert, CircleCheck, Info, X } from "lucide-react";
import { useT } from "../i18n";
import { useToasts } from "../state/toasts";

export function Toasts() {
  const { toasts, dismiss } = useToasts();
  const { t } = useT();
  return (
    <div className="toasts" role="region" aria-label={t("Notifications")} aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.kind}`} role={toast.kind === "error" ? "alert" : "status"}>
          {toast.kind === "error" ? <CircleAlert size={16} /> : toast.kind === "success" ? <CircleCheck size={16} /> : <Info size={16} />}
          <div className="toast-text">
            <div className="toast-title">{toast.title}</div>
            {toast.detail && <div className="toast-detail">{toast.detail}</div>}
          </div>
          <button type="button" className="toast-close" aria-label={t("Dismiss")} onClick={() => dismiss(toast.id)}>
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
