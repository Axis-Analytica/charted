// The UI talks to the Rust core through a Transport, so the same code runs inside the
// Tauri window, in a plain browser against the development server, and in tests.
import type { AppState } from "./generated/AppState";
import type { DataRequest } from "./generated/DataRequest";
import type { EnvSnapshot } from "./generated/EnvSnapshot";
import type { Request } from "./generated/Request";

export interface PickOptions {
  title: string;
  /** Pick a folder instead of a file. */
  directory?: boolean;
  /** Offer to create a new folder (used by "New project here"). */
  create?: boolean;
  filters?: { name: string; extensions: string[] }[];
  /** Choose where to save a new file, suggesting this name. */
  saveAs?: string;
}

export interface Transport {
  rpc(request: Request): Promise<AppState>;
  data(request: DataRequest): Promise<ArrayBuffer>;
  env(): Promise<EnvSnapshot>;
  onEnv(callback: (env: EnvSnapshot) => void): () => void;
  /** States the core pushes after changes the UI didn't ask for (the agent's). */
  onState(callback: (state: AppState) => void): () => void;
  pick(options: PickOptions): Promise<string | null>;
  /** Paths passed on the command line (`charted open <path>`), consumed once. */
  initialPath(): Promise<string | null>;
  quit(): Promise<void>;
  /** Opens a new GitHub issue for charted (a crash report) in the browser. */
  openIssue(url: string): Promise<void>;
}

export interface ApiError {
  code: string;
  message: string;
  /** What the error is about, when the UI can offer a fix (an extension, a connection). */
  detail?: string | null;
}

export function isApiError(e: unknown): e is ApiError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isApiError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}
