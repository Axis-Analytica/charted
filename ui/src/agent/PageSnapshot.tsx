// Draws a page for the agent's screenshot_page: off screen, at the page's own size (at
// most 1280 px wide), once every visual has loaded, then sends it back as a PNG.
import { toPng } from "html-to-image";
import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { errorMessage, transport } from "../bridge";
import type { Model } from "../bridge/generated/Model";
import { usePageTheme } from "../report/Canvas";
import { modelSignature } from "../report/data";
import { useAllLoaded } from "../report/PrintLayout";
import { ThemeKey } from "../report/scale";
import { VisualContainer } from "../report/VisualContainer";
import { useAgent } from "../state/agent";

const noop = () => {};

export function PageSnapshot({ model }: { model: Model }) {
  const job = useAgent((s) => s.snapshot);
  // A fresh drawing for each request, so "loaded" is about this page.
  return job ? <Shot key={job.id} job={job} model={model} /> : null;
}

function Shot({ job, model }: { job: { id: string; page: string }; model: Model }) {
  const root = useRef<HTMLDivElement>(null);
  const theme = usePageTheme(model);
  const ready = useAllLoaded(root, true);
  const page = model.report.pages.find((p) => p.id === job.page);
  const done = useRef(false);

  useEffect(() => {
    if (done.current) return;
    const answer = (png: string | null, error: string | null) => {
      done.current = true;
      useAgent.setState({ snapshot: null });
      void transport.agent({ type: "snapshot", id: job.id, png, error }).catch(() => {});
    };
    if (!page) return answer(null, "there's no such page");
    if (!ready || !root.current) return;
    const scale = Math.min(1, 1280 / page.width);
    toPng(root.current, { pixelRatio: scale, cacheBust: false, skipFonts: false })
      .then((url) => answer(url.slice(url.indexOf(",") + 1), null))
      .catch((e) => answer(null, `the page couldn't be drawn: ${errorMessage(e)}`));
  }, [job, page, ready]);

  if (!page) return null;
  return createPortal(
    <div className="agent-snapshot" aria-hidden>
      <ThemeKey.Provider value={theme.key}>
        <div ref={root} className="page" data-themed={theme.tokens ? "" : undefined} style={{ ...theme.style, width: page.width, height: page.height }}>
          {page.visuals
            .filter((v) => !v.hidden)
            .map((v) => (
              <VisualContainer key={v.id} visual={v} page={page} model={model} signature={modelSignature(model)} selected={false} register={noop} onPointerDown={noop} />
            ))}
        </div>
      </ThemeKey.Provider>
    </div>,
    document.body,
  );
}
