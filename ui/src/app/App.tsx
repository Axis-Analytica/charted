import { useEffect, useRef, useState } from "react";
import { transport } from "../bridge";
import { applyTokens, deriveTokens } from "../design/theme";
import { useLanguage, useT } from "../i18n";
import { ConfirmDialog } from "../kit/Dialog";
import { Spinner } from "../kit/misc";
import { Toasts } from "../kit/Toasts";
import { useApp } from "../state/app";
import { KitGallery } from "./KitGallery";
import { CommandPalette, ShortcutOverlay } from "./Overlays";
import { installShortcutHandler, useShortcut } from "./shortcuts";
import { ProjectView } from "../views/ProjectView";
import { Welcome } from "../views/Welcome";

/** Applies the desktop environment (theme, rounding, motion, language, font) live. */
function useEnvironment() {
  const setEnv = useApp((s) => s.setEnv);
  useEffect(() => {
    const apply = (env: Awaited<ReturnType<typeof transport.env>>) => {
      setEnv(env);
      applyTokens(deriveTokens(env.theme, env.desktop), env.theme.mode, env.desktop.animations);
      if (env.monoFont) document.documentElement.style.setProperty("--omarchy-mono", `"${env.monoFont}"`);
      useLanguage.getState().set(env.language.code, env.language.formatLocale, env.language.rtl);
    };
    transport.env().then(apply);
    return transport.onEnv(apply);
  }, [setEnv]);
}

export function App() {
  useEnvironment();
  const { t } = useT();
  const state = useApp((s) => s.state);
  const busy = useApp((s) => s.busy);
  const rpc = useApp((s) => s.rpc);
  const [help, setHelp] = useState(false);
  const [palette, setPalette] = useState(false);
  const [quitAsk, setQuitAsk] = useState(false);
  const [kit, setKit] = useState(false);
  const kitOpen = useRef(false);
  kitOpen.current = kit;
  const dirty = !!state?.project?.dirty;

  useEffect(() => installShortcutHandler(), []);
  useEffect(() => {
    (async () => {
      await rpc({ type: "state" });
      const path = await transport.initialPath();
      if (path) await rpc({ type: "openProject", path }, t("Opening project…"));
    })();
  }, [rpc, t]);

  const quit = () => (useApp.getState().state?.project?.dirty ? setQuitAsk(true) : transport.quit());
  useEffect(() => {
    // The shell asks before closing the window (e.g. Super+W) when work is unsaved.
    const onClose = () => quit();
    const onPalette = () => setPalette(true);
    const onHelp = () => setHelp(true);
    window.addEventListener("charted:close-requested", onClose);
    window.addEventListener("charted:palette", onPalette);
    window.addEventListener("charted:help", onHelp);
    return () => {
      window.removeEventListener("charted:close-requested", onClose);
      window.removeEventListener("charted:palette", onPalette);
      window.removeEventListener("charted:help", onHelp);
    };
  });

  useShortcut(
    { id: "help", keys: "?", label: "Show keyboard shortcuts", group: "General", run: () => setHelp(true) },
    { id: "palette", keys: "Ctrl+K", label: "Command palette", group: "General", run: () => setPalette(true), global: true },
    { id: "quit", keys: "Q", label: "Quit", group: "General", run: quit },
    { id: "kit", keys: "Ctrl+Alt+K", label: "Component kit", group: "View", run: () => setKit((k) => !k), global: true },
    { id: "kit-close", keys: "Escape", label: "", group: "View", run: () => setKit(false), enabled: () => kitOpen.current },
  );

  return (
    <>
      {state === null ? (
        <div className="splash">
          <Spinner size={20} label={t("Starting")} />
        </div>
      ) : state.project ? (
        <ProjectView project={state.project} />
      ) : (
        <Welcome state={state} />
      )}
      {busy && (
        <div className="busy" role="status">
          <Spinner size={18} />
          <span>{busy}</span>
        </div>
      )}
      {kit && <KitGallery onClose={() => setKit(false)} />}
      <ShortcutOverlay open={help} onClose={() => setHelp(false)} />
      <CommandPalette open={palette} onClose={() => setPalette(false)} />
      <ConfirmDialog
        open={quitAsk}
        title={t("Save changes before quitting?")}
        message={t("Your changes to %1 will be lost if you don't save them.", state?.project?.name ?? "")}
        confirm={t("Save and quit")}
        alternative={t("Quit without saving")}
        cancel={t("Cancel")}
        onCancel={() => setQuitAsk(false)}
        onAlternative={() => transport.quit()}
        onConfirm={async () => {
          if (await rpc({ type: "save" })) transport.quit();
        }}
      />
      <Toasts />
      {dirty && <span className="visually-hidden">{t("Unsaved changes")}</span>}
    </>
  );
}
