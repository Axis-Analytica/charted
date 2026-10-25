import { createContext } from "react";

/** The canvas zoom, so charts can render at enough pixels to stay sharp. */
export const CanvasScale = createContext(1);

/** Changes when the report theme does, so canvas charts re-read its colours. */
export const ThemeKey = createContext("");
