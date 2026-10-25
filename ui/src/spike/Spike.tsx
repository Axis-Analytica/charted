import { useState } from "react";
import { ArrowSpike } from "./ArrowSpike";
import { ChartSpike } from "./ChartSpike";
import { EditorSpike } from "./EditorSpike";
import { FrameMeter } from "../app/FrameMeter";
import { GridSpike } from "./GridSpike";
import "./spike.css";

const tabs = { grid: GridSpike, chart: ChartSpike, editor: EditorSpike, arrow: ArrowSpike } as const;
type Tab = keyof typeof tabs;

/** Throwaway page that de-risks the UI stack on WebKitGTK before the real shell exists. */
export function Spike() {
  const initial = ((new URLSearchParams(location.search).get("tab") ?? import.meta.env.VITE_SPIKE_TAB) as Tab | undefined) ?? "grid";
  const [tab, setTab] = useState<Tab>(initial in tabs ? initial : "grid");
  const View = tabs[tab];
  return (
    <div className="spike">
      <header>
        <strong>charted · WebKitGTK spike</strong>
        <nav>
          {(Object.keys(tabs) as Tab[]).map((t) => (
            <button type="button" key={t} aria-pressed={t === tab} onClick={() => setTab(t)}>
              {t}
            </button>
          ))}
        </nav>
        <FrameMeter />
      </header>
      <main>
        <View />
      </main>
    </div>
  );
}
