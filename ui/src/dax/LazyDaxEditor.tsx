// The DAX editor (Monaco) loads on first use, so it isn't part of the initial
// download: the report opens without it.
import { lazy, Suspense, type ComponentProps } from "react";
import type { DaxEditor as Editor } from "./DaxEditor";

export type { DaxEditorHandle } from "./DaxEditor";

const load = () => import("./DaxEditor");
const Loaded = lazy(() => load().then((m) => ({ default: m.DaxEditor })));

/** Starts loading the editor in the background (once a project is open), so it's ready
 * before the first formula without being part of the initial download. */
export function preloadDaxEditor() {
  const start = () => void load();
  if ("requestIdleCallback" in window) window.requestIdleCallback(start, { timeout: 3000 });
  else setTimeout(start, 1500);
}

export function DaxEditor(props: ComponentProps<typeof Editor>) {
  return (
    <Suspense fallback={<div className="dax-editor-loading" aria-label={props.label} aria-busy="true" />}>
      <Loaded {...props} />
    </Suspense>
  );
}
