// Buttons and page navigators. In reading view a click acts; while editing a click
// selects the visual and Ctrl+click acts (the canvas leaves Ctrl+click on these to them).
import { ArrowLeft, Bookmark, ChevronsRight, CornerDownRight, Info } from "lucide-react";
import type { CSSProperties, MouseEvent } from "react";
import type { Model } from "../../bridge/generated/Model";
import type { Page } from "../../bridge/generated/Page";
import type { Visual } from "../../bridge/generated/Visual";
import { useT } from "../../i18n";
import { useReport } from "../../state/report";
import { buttonAction, goToPage, navigatorPages, runAction } from "../actions";
import { prop, visualDef } from "../registry";
import "../building.css";

const ICONS = { back: ArrowLeft, bookmark: Bookmark, pageNavigation: ChevronsRight, drillthrough: CornerDownRight, information: Info } as const;

/** Whether a click on a button acts now: always in reading view, with Ctrl while editing. */
const acts = (e: MouseEvent, reading: boolean) => reading || e.ctrlKey || e.metaKey;

export function ActionButton({ visual, page, model }: { visual: Visual; page: Page; model: Model }) {
  const { t } = useT();
  const reading = useReport((s) => s.reading);
  const def = visualDef("actionButton");
  const p = (object: string, name: string) => prop<unknown>(visual.objects, def, object, name);
  const kind = String(p("button", "kind"));
  const Icon = ICONS[kind as keyof typeof ICONS];
  const text = String(p("button", "text") ?? "");
  const action = buttonAction(visual);
  const tip = action.tooltip || (reading ? "" : t("Ctrl+click to use the button"));
  const style = {
    "--btn-fill": (p("button", "fill") as string) || undefined,
    "--btn-color": (p("button", "color") as string) || undefined,
    "--btn-hover-fill": (p("hover", "fill") as string) || undefined,
    "--btn-hover-color": (p("hover", "color") as string) || undefined,
    "--btn-pressed-fill": (p("pressed", "fill") as string) || undefined,
    "--btn-pressed-color": (p("pressed", "color") as string) || undefined,
    fontSize: Number(p("button", "fontSize")),
  } as CSSProperties;
  return (
    <button
      type="button"
      className={`action-button ${p("button", "border") ? "has-border" : ""} ${p("button", "rounded") ? "is-rounded" : ""} ${reading ? "is-live" : ""}`}
      data-action={action.type}
      style={style}
      title={tip || undefined}
      aria-label={text || tip || t("Button")}
      onClick={(e) => {
        if (!acts(e, reading)) return;
        e.stopPropagation();
        runAction(model, page, action, t);
      }}
    >
      {Icon && <Icon size={Math.round(Number(p("button", "fontSize")) * 1.2)} aria-hidden />}
      {text && <span>{text}</span>}
    </button>
  );
}

export function PageNavigator({ visual, page, model }: { visual: Visual; page: Page; model: Model }) {
  const { t } = useT();
  const reading = useReport((s) => s.reading);
  const def = visualDef("pageNavigator");
  const p = (name: string) => prop<unknown>(visual.objects, def, "navigator", name);
  const pages = navigatorPages(model, !!p("showHidden"));
  const style = {
    "--nav-fill": (p("fill") as string) || undefined,
    "--nav-selected": (p("selectedFill") as string) || undefined,
    "--nav-color": (p("color") as string) || undefined,
    fontSize: Number(p("fontSize")),
  } as CSSProperties;
  return (
    <nav className={`page-navigator is-${String(p("orientation"))} ${reading ? "is-live" : ""}`} style={style} aria-label={t("Page navigator")}>
      {pages.map((x) => (
        <button
          key={x.id}
          type="button"
          className={`page-nav-button ${x.id === page.id ? "is-current" : ""}`}
          aria-current={x.id === page.id ? "page" : undefined}
          title={reading ? undefined : t("Ctrl+click to go to %1", x.name)}
          onClick={(e) => {
            if (!acts(e, reading)) return;
            e.stopPropagation();
            goToPage(x.id);
          }}
        >
          {x.name}
        </button>
      ))}
    </nav>
  );
}
