// Report fonts: fifteen bundled open fonts (SIL Open Font License, shipped with charted
// so reports look the same everywhere), the fonts the user adds (their library and the
// open project's copies, loaded from the core as web fonts), and Power BI's usual font
// names, each with a bundled stand-in for when it isn't installed.
import "@fontsource-variable/roboto";
import "@fontsource-variable/open-sans";
import "@fontsource/lato/latin-400.css";
import "@fontsource/lato/latin-400-italic.css";
import "@fontsource/lato/latin-700.css";
import "@fontsource-variable/montserrat";
import "@fontsource/poppins/latin-400.css";
import "@fontsource/poppins/latin-400-italic.css";
import "@fontsource/poppins/latin-700.css";
import "@fontsource-variable/source-sans-3";
import "@fontsource-variable/noto-sans";
import "@fontsource-variable/nunito";
import "@fontsource-variable/ibm-plex-sans";
import "@fontsource-variable/oswald";
import "@fontsource-variable/merriweather";
import "@fontsource-variable/lora";
import "@fontsource-variable/playfair-display";
import "@fontsource-variable/roboto-mono";
import { create } from "zustand";
import { quote } from "./fontNames";
import type { FontFile } from "../bridge/generated/FontFile";
import { transport } from "../bridge";

export { BUNDLED, fontStack, POWER_BI } from "./fontNames";

type Fonts = {
  /** The user's fonts' families (library and project), for pickers. */
  families: string[];
  loaded: Set<string>;
};

export const useFonts = create<Fonts>()(() => ({ families: [], loaded: new Set() }));

const toDataUrl = (bytes: ArrayBuffer, file: string) =>
  new Promise<string>((resolve, reject) => {
    const ext = file.split(".").pop()?.toLowerCase();
    const type = ext === "woff2" ? "font/woff2" : ext === "otf" ? "font/otf" : "font/ttf";
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsDataURL(new Blob([bytes], { type }));
  });

/** Registers the user's fonts as web fonts, each once. They go in a stylesheet (not the
 * FontFace API) so a page drawn to a picture for the agent carries them. */
export async function loadFonts(fonts: FontFile[]) {
  const families = [...new Set(fonts.map((f) => f.family))].sort((a, b) => a.localeCompare(b));
  const { loaded } = useFonts.getState();
  if (families.join("\n") !== useFonts.getState().families.join("\n")) useFonts.setState({ families });
  let sheet = document.getElementById("charted-user-fonts") as HTMLStyleElement | null;
  for (const f of fonts) {
    if (loaded.has(f.file)) continue;
    loaded.add(f.file);
    try {
      const url = await toDataUrl(await transport.data({ type: "readFont", file: f.file }), f.file);
      if (!sheet) {
        sheet = document.createElement("style");
        sheet.id = "charted-user-fonts";
        document.head.append(sheet);
      }
      sheet.append(`@font-face { font-family: ${quote(f.family)}; src: url(${url}); font-weight: ${f.weight}; font-style: ${f.italic ? "italic" : "normal"}; font-display: swap; }\n`);
    } catch {
      loaded.delete(f.file);
    }
  }
}

/** Asks for a font file and adds it to the user's library; returns its family. */
export async function addFont(title: string): Promise<string | null> {
  const path = await transport.pick({ title, filters: [{ name: "Fonts", extensions: ["ttf", "otf", "woff2"] }] });
  if (!path) return null;
  const { useApp } = await import("../state/app");
  const before = new Set(useApp.getState().state?.fonts.map((f) => f.file));
  if (!(await useApp.getState().rpc({ type: "addFont", path }))) return null;
  const added = useApp.getState().state?.fonts.find((f) => !before.has(f.file) || path.endsWith(f.file));
  return added?.family ?? null;
}
