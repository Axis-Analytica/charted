// Export to PDF: every page not hidden, one per sheet at the page's size, with its
// current filters and slicers. The print layout stacks the pages (CSS `@page` sized to
// the report page, a page break after each), waits until every visual has loaded, then
// the desktop app prints it to a PDF file through WebKitGTK; in a browser it offers the
// browser's print dialog. `charted open <project> --export-pdf FILE` runs the same.
import { Printer, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { create } from "zustand";
import { errorMessage, transport } from "../bridge";
import type { Model } from "../bridge/generated/Model";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { useApp } from "../state/app";
import { useReport } from "../state/report";
import { toast } from "../state/toasts";
import { usePageTheme } from "./Canvas";
import { modelSignature } from "./data";
import { CanvasScale, ThemeKey } from "./scale";
import { VisualContainer } from "./VisualContainer";
import "./building.css";

export interface PrintJob {
  /** The PDF to write (desktop app), or null to show the layout with Print (browser). */
  path: string | null;
  /** Started from the command line: exit when done. */
  cli?: boolean;
}

export const usePrint = create<{ job: PrintJob | null; set(job: PrintJob | null): void }>((set) => ({ job: null, set: (job) => set({ job }) }));

type Desktop = { printPdf?(path: string, width: number, height: number): Promise<void>; exitWith?(code: number): Promise<void> };
const desktop = () => transport as unknown as Desktop;

/** Whether this window can write PDFs itself (the desktop app). */
export const canPrintToFile = () => typeof desktop().printPdf === "function";

/** Starts File → Export → PDF. */
export async function exportPdf(t: (s: string) => string) {
  let path: string | null = null;
  if (canPrintToFile()) {
    const name = (useApp.getState().state?.project?.name ?? "Report").replace(/[/\\?%*:|"<>]/g, "-");
    path = await transport.pick({ title: t("Export to PDF"), saveAs: `${name}.pdf`, filters: [{ name: "PDF", extensions: ["pdf"] }] });
    if (!path) return;
  }
  useReport.getState().set({ view: "report", focus: null });
  usePrint.getState().set({ path });
}

// The command line's entry point (`--export-pdf`): the app calls it once the project is open.
(window as unknown as { __chartedExportPdf?: (path: string) => void }).__chartedExportPdf = (path: string) => {
  useReport.getState().set({ view: "report", focus: null });
  usePrint.getState().set({ path, cli: true });
};

/** Polls until no visual is loading (several checks in a row), so charts are drawn. */
function useAllLoaded(root: React.RefObject<HTMLElement | null>, active: boolean) {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (!active) return;
    let calm = 0;
    let stop = false;
    const tick = async () => {
      await document.fonts?.ready;
      while (!stop) {
        const busy = root.current?.querySelector("[data-loading]");
        calm = busy || !root.current ? 0 : calm + 1;
        if (calm >= 4) return setReady(true);
        await new Promise((r) => setTimeout(r, 150));
      }
    };
    void tick();
    return () => {
      stop = true;
    };
  }, [root, active]);
  return ready;
}

export function PrintLayout({ model }: { model: Model }) {
  const { t } = useT();
  const job = usePrint((s) => s.job);
  const root = useRef<HTMLDivElement>(null);
  const theme = usePageTheme(model);
  const signature = modelSignature(model);
  const pages = model.report.pages.filter((p) => !p.hidden);
  // Every sheet takes the first page's size; other sizes are scaled to fit it.
  const width = pages[0]?.width ?? 1280;
  const height = pages[0]?.height ?? 720;
  const ready = useAllLoaded(root, !!job);
  const printed = useRef(false);

  // Charts draw their final frame at once.
  useEffect(() => {
    if (!job) return;
    const root = document.documentElement;
    const before = root.dataset.motion;
    root.dataset.motion = "off";
    root.classList.add("is-printing");
    return () => {
      root.dataset.motion = before;
      root.classList.remove("is-printing");
    };
  }, [job]);

  useEffect(() => {
    if (!job || !ready || printed.current || !job.path) return;
    printed.current = true;
    const path = job.path;
    (async () => {
      let ok = true;
      try {
        await desktop().printPdf!(path, width, height);
        if (!job.cli) toast({ kind: "success", title: t("Exported the report to PDF"), detail: path });
      } catch (e) {
        ok = false;
        if (job.cli) console.error(errorMessage(e));
        else toast({ kind: "error", title: t("Couldn't export to PDF"), detail: errorMessage(e) });
      }
      usePrint.getState().set(null);
      if (job.cli) await desktop().exitWith?.(ok ? 0 : 1);
    })();
  }, [job, ready, width, height, t]);
  useEffect(() => {
    if (!job) printed.current = false;
  }, [job]);

  if (!job) return null;
  return createPortal(
    <div ref={root} className="print-root" data-print-ready={ready ? "" : undefined} data-pages={pages.length} aria-label={t("Export to PDF")} role="dialog">
      <style>{`@page { size: ${width}px ${height}px; margin: 0; }`}</style>
      <div className="print-bar">
        <span>{ready ? t("%1 pages ready", pages.length) : t("Preparing the PDF…")}</span>
        <span className="status-fill" />
        {!job.path && (
          <Button variant="primary" size="sm" icon={<Printer size={14} />} isDisabled={!ready} onPress={() => window.print()}>
            {t("Print…")}
          </Button>
        )}
        <Button variant="ghost" size="sm" icon={<X size={14} />} onPress={() => usePrint.getState().set(null)}>
          {t("Close")}
        </Button>
      </div>
      <CanvasScale.Provider value={2}>
        <ThemeKey.Provider value={theme.key}>
          {pages.map((p) => {
            const fit = Math.min(width / p.width, height / p.height);
            return (
              <section key={p.id} className="print-page" style={{ width, height }} aria-label={p.name}>
                <div className="page" data-themed={theme.tokens ? "" : undefined} style={{ ...theme.style, width: p.width, height: p.height, transform: fit === 1 ? undefined : `scale(${fit})`, transformOrigin: "0 0" }}>
                  {p.visuals
                    .filter((v) => !v.hidden)
                    .map((v) => (
                      <VisualContainer key={v.id} visual={v} page={p} model={model} signature={signature} selected={false} register={noop} onPointerDown={noop} />
                    ))}
                </div>
              </section>
            );
          })}
        </ThemeKey.Provider>
      </CanvasScale.Provider>
    </div>,
    document.body,
  );
}

const noop = () => {};
