// A table or matrix cell under conditional formatting: its colours, data bar and icon.
import { ArrowDown, ArrowRight, ArrowUp, CircleAlert, CircleCheck, CircleX, Flag, Star } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { type CellLook, type CfIcon, contrastText } from "./conditional";

export const CF_ICONS: Record<CfIcon, typeof ArrowUp> = { up: ArrowUp, right: ArrowRight, down: ArrowDown, good: CircleCheck, warn: CircleAlert, bad: CircleX, flag: Flag, star: Star };

/** The cell's style (background and font colour). */
export function lookStyle(look: CellLook | null): CSSProperties | undefined {
  if (!look || (!look.background && !look.color)) return undefined;
  return { background: look.background, color: look.color ?? (look.background ? contrastText(look.background) : undefined) };
}

/** The cell's content: the text, with its icon and data bar. */
export function CfContent({ look, children }: { look: CellLook | null; children: ReactNode }) {
  if (!look || (!look.bar && !look.icon)) return <>{children}</>;
  const Icon = look.icon ? CF_ICONS[look.icon.name] : null;
  return (
    <>
      {look.bar && <span className="cf-bar" data-cf-bar style={{ insetInlineStart: `${look.bar.from}%`, width: `${Math.max(0.5, look.bar.to - look.bar.from)}%`, background: look.bar.color, opacity: 0.55 }} />}
      <span className="cf-text">
        {Icon && <Icon size={12} className="cf-icon" data-cf-icon={look.icon!.name} style={{ color: look.icon!.color }} />}
        {look.hideValue ? null : children}
      </span>
    </>
  );
}
