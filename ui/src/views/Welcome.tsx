import { ChartColumnBig, Clock, FileInput, FolderOpen, Sparkles, TriangleAlert } from "lucide-react";
import { transport } from "../bridge";
import type { AppState } from "../bridge/generated/AppState";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Kbd } from "../kit/Kbd";
import { useApp } from "../state/app";
import { useShortcut } from "../app/shortcuts";

function relativeTime(unixSeconds: number, locale: string): string {
  const diff = unixSeconds - Date.now() / 1000;
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const abs = Math.abs(diff);
  if (abs < 60) return rtf.format(Math.round(diff), "second");
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), "hour");
  return rtf.format(Math.round(diff / 86400), "day");
}

export function Welcome({ state }: { state: AppState }) {
  const { t } = useT();
  const rpc = useApp((s) => s.rpc);
  const locale = useApp((s) => s.env?.language.formatLocale ?? "en-US");
  const openDialog = async () => {
    const path = await transport.pick({ title: t("Open project"), directory: true });
    if (path) await rpc({ type: "openProject", path }, t("Opening project…"));
  };
  const openFile = async () => {
    const path = await transport.pick({ title: t("Open a file"), filters: [{ name: t("charted bundle or Power BI project"), extensions: ["chartedz", "pbip", "bim"] }] });
    if (path) await rpc({ type: "openProject", path }, t("Opening…"));
  };
  const openSample = () => rpc({ type: "openSample", dir: null }, t("Preparing the Harbor & Pine sample…"));
  useShortcut(
    { id: "open", keys: "Ctrl+O", label: "Open project", group: "File", run: openDialog },
    { id: "sample", keys: "", label: "Open the sample project", group: "File", run: openSample },
  );

  return (
    <div className="welcome">
      <div className="welcome-inner">
        <div className="welcome-brand">
          <ChartColumnBig size={34} strokeWidth={1.75} className="welcome-mark" />
          <div>
            <h1>charted</h1>
            <p>{t("Load data, model it, and build interactive reports. Locally.")}</p>
          </div>
        </div>
        <div className="welcome-actions">
          <Button variant="primary" size="lg" icon={<FolderOpen size={17} />} onPress={openDialog}>
            {t("Open project…")}
          </Button>
          <Button size="lg" icon={<FileInput size={17} />} onPress={openFile}>
            {t("Open a bundle or Power BI project…")}
          </Button>
          <Button size="lg" icon={<Sparkles size={17} />} onPress={openSample}>
            {t("Explore the sample")}
          </Button>
        </div>
        <section className="welcome-recent" aria-labelledby="recent-title">
          <h2 id="recent-title">
            <Clock size={14} /> {t("Recent")}
          </h2>
          {state.recent.length === 0 ? (
            <p className="welcome-none">{t("Projects you open will appear here.")}</p>
          ) : (
            <ul>
              {state.recent.map((r) => (
                <li key={r.path}>
                  <button
                    type="button"
                    className="recent-item"
                    disabled={!r.exists}
                    onClick={() => rpc({ type: "openProject", path: r.path }, t("Opening project…"))}
                  >
                    <span className="recent-name">{r.name}</span>
                    <span className="recent-path" title={r.path}>
                      {r.exists ? r.path : (
                        <>
                          <TriangleAlert size={12} /> {t("Moved or deleted")}
                        </>
                      )}
                    </span>
                    <span className="recent-time">{relativeTime(r.openedAt, locale)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
        <footer className="welcome-foot">
          <span>
            <Kbd keys="Ctrl+O" /> {t("Open")}
          </span>
          <span>
            <Kbd keys="Ctrl+K" /> {t("Commands")}
          </span>
          <span>
            <Kbd keys="?" /> {t("Shortcuts")}
          </span>
          <span className="welcome-version">v{state.version}</span>
        </footer>
      </div>
    </div>
  );
}
