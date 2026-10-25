import type { ReactNode } from "react";

export function EmptyState({ icon, title, children, actions }: { icon?: ReactNode; title: string; children?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="empty">
      {icon && <div className="empty-icon">{icon}</div>}
      <div className="empty-title">{title}</div>
      {children && <div className="empty-text">{children}</div>}
      {actions && <div className="empty-actions">{actions}</div>}
    </div>
  );
}

export function Spinner({ size = 16, label }: { size?: number; label?: string }) {
  return <span className="spinner" style={{ width: size, height: size }} role="progressbar" aria-label={label} />;
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "accent" | "danger" | "warning" }) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}

export function Banner({ tone, children, actions }: { tone: "info" | "warning" | "danger"; children: ReactNode; actions?: ReactNode }) {
  return (
    <div className={`banner banner-${tone}`} role={tone === "danger" ? "alert" : "status"}>
      <div className="banner-text">{children}</div>
      {actions && <div className="banner-actions">{actions}</div>}
    </div>
  );
}
