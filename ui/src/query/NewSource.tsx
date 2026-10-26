// Get Data: new queries from files, folders and databases, the Excel navigator, and
// the prompts a database source can need (agreeing to download DuckDB's extension, a
// password for the keyring).
import { useEffect, useMemo, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { Query } from "../bridge/generated/Query";
import type { Source } from "../bridge/generated/Source";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { Spinner } from "../kit/misc";
import { useApp } from "../state/app";
import { toast } from "../state/toasts";
import { analyze, clearQueryCache, usePreview } from "./data";
import { headerNames } from "./transforms";
import type { DataType } from "../bridge/generated/DataType";
import { PreviewTable } from "./PreviewTable";
import { newId } from "../state/report";
import { tableName, uniqueQueryName } from "./state";

export type SourceKind = "csv" | "excel" | "json" | "parquet" | "folder" | "sqlite" | "postgres";

/** The project's folder (the snapshot's path is its `.charted` file). */
export function projectDir(manifest: string): string {
  return manifest.replace(/\/[^/]*\.charted$/, "");
}

/** A path inside the project folder is stored relative to it, so the project moves.
 * `project` is the project's `.charted` file or its folder. */
export function projectPath(project: string, path: string): string {
  const dir = projectDir(project).replace(/\/+$/, "");
  return path.startsWith(`${dir}/`) ? path.slice(dir.length + 1) : path;
}

const stem = (p: string) => (p.split("/").pop() ?? p).replace(/\.[^.]+$/, "");

export function newQuery(name: string, source: Source, queries: Query[]): Query {
  const n = uniqueQueryName(name, queries);
  return { id: newId(), name: n, table: tableName(n, queries), source, steps: [], stepNames: {}, noLoad: false };
}

const FILTERS: Record<string, { name: string; extensions: string[] }[]> = {
  csv: [{ name: "CSV", extensions: ["csv", "tsv", "txt"] }],
  excel: [{ name: "Excel", extensions: ["xlsx", "xlsm", "xls", "ods"] }],
  json: [{ name: "JSON", extensions: ["json", "jsonl", "ndjson"] }],
  parquet: [{ name: "Parquet", extensions: ["parquet"] }],
  sqlite: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
};

/** Starts getting data of a kind: picks a file or folder, or opens a form. */
export async function pickSource(kind: SourceKind, project: string, queries: Query[], t: (s: string) => string): Promise<{ query?: Query; dialog?: SourceDialogState } | null> {
  if (kind === "postgres") return { dialog: { kind: "postgres" } };
  const path = await transport.pick({ title: t("Get data"), directory: kind === "folder", filters: FILTERS[kind] ?? [] });
  if (!path) return null;
  const rel = projectPath(project, path);
  switch (kind) {
    case "csv": {
      // What reading the file's start found is kept with the query, so its settings
      // show it and Power BI reads the file the same way.
      const found = await transport
        .data({ type: "detectCsv", path: rel })
        .then((b) => JSON.parse(new TextDecoder().decode(b)) as { encoding: string | null; decimal: string | null })
        .catch(() => ({ encoding: null, decimal: null }));
      return { query: await withHeadersAndTypes(newQuery(stem(path), { kind: "csv", path: rel, delimiter: null, header: false, encoding: found.encoding, decimal: found.decimal }, queries)) };
    }
    case "json":
      return { query: newQuery(stem(path), { kind: "json", path: rel }, queries) };
    case "parquet":
      return { query: newQuery(stem(path), { kind: "parquet", path: rel }, queries) };
    case "excel":
      return { dialog: { kind: "excel", path: rel } };
    case "folder":
      return { dialog: { kind: "folder", path: rel } };
    case "sqlite":
      return { dialog: { kind: "sqlite", path: rel } };
  }
}

/** A text file's first steps, as Power Query adds them: the first row as headers, then
 * the types its values read as. */
async function withHeadersAndTypes(q: Query): Promise<Query> {
  try {
    const row = (await analyze(q, [], 0, { kind: "firstRow" })) as (string | null)[];
    const withHeaders: Query = { ...q, steps: [{ op: "promoteHeaders", names: headerNames(row) }] };
    const types = (await analyze(withHeaders, [], 1, { kind: "detectTypes" })) as [string, DataType][];
    const typed = types.filter(([, ty]) => ty !== "String");
    return typed.length ? { ...withHeaders, steps: [...withHeaders.steps, { op: "changeTypes", columns: typed }] } : withHeaders;
  } catch {
    // The editor shows what's wrong with the file.
    return q;
  }
}

export type SourceDialogState = { kind: "excel"; path: string } | { kind: "folder"; path: string } | { kind: "sqlite"; path: string } | { kind: "postgres" };

function ExcelNavigator({ path, queries, onDone }: { path: string; queries: Query[]; onDone(qs: Query[] | null): void }) {
  const { t } = useT();
  const [sheets, setSheets] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [shown, setShown] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    transport
      .data({ type: "excelSheets", path })
      .then((b) => {
        const s = JSON.parse(new TextDecoder().decode(b)) as string[];
        if (!live) return;
        setSheets(s);
        setShown(s[0] ?? null);
      })
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [path]);
  // A stable query per sheet (a new id each render would ask again every render).
  const draft = useMemo<Query | null>(
    () => (shown ? { id: `navigator:${shown}`, name: shown, table: "navigator", source: { kind: "excel", path, sheet: shown, header: true }, steps: [], stepNames: {}, noLoad: false } : null),
    [shown, path],
  );
  const preview = usePreview(draft, [], 0);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone(null)}
      title={t("Navigator")}
      width={860}
      footer={
        <>
          <Button variant="ghost" onPress={() => onDone(null)}>
            {t("Cancel")}
          </Button>
          <Button
            variant="primary"
            isDisabled={!chosen.length}
            onPress={() => {
              const made: Query[] = [];
              for (const s of chosen) made.push(newQuery(s, { kind: "excel", path, sheet: s, header: true }, [...queries, ...made]));
              onDone(made);
            }}
          >
            {t("Load")}
          </Button>
        </>
      }
    >
      <div className="qe-navigator">
        <div className="qe-nav-list" role="listbox" aria-label={t("Sheets")}>
          <div className="qe-nav-file">{path.split("/").pop()}</div>
          {error && <p className="qe-error">{error}</p>}
          {!sheets && !error && <Spinner />}
          {sheets?.map((s) => (
            <label key={s} className={`qe-nav-item ${shown === s ? "is-on" : ""}`} onMouseEnter={() => setShown(s)} onFocus={() => setShown(s)}>
              <input type="checkbox" checked={chosen.includes(s)} onChange={(e) => setChosen((c) => (e.target.checked ? [...c, s] : c.filter((x) => x !== s)))} />
              <span onClick={() => setShown(s)}>{s}</span>
            </label>
          ))}
        </div>
        <div className="qe-nav-preview">
          {preview.error ? <p className="qe-error">{preview.error.message}</p> : preview.data ? <PreviewTable preview={preview.data} limit={30} /> : <Spinner />}
        </div>
      </div>
    </Dialog>
  );
}

function SourceForm({ state, queries, onDone }: { state: Exclude<SourceDialogState, { kind: "excel" }>; queries: Query[]; onDone(q: Query | null): void }) {
  const { t } = useT();
  const [v, setV] = useState<Record<string, string>>(
    state.kind === "folder" ? { pattern: "*.csv" } : state.kind === "sqlite" ? { table: "" } : { host: "localhost", port: "5432", database: "", user: "", password: "", schema: "public", table: "" },
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = (k: string, label: string, type = "text") => (
    <label className="field">
      <span className="field-label">{label}</span>
      <input className="input" type={type} aria-label={label} value={v[k] ?? ""} onChange={(e) => setV((o) => ({ ...o, [k]: e.target.value }))} onKeyDown={(e) => e.stopPropagation()} />
    </label>
  );
  const ok = async () => {
    setError(null);
    if (state.kind === "folder") return onDone(newQuery(stem(state.path), { kind: "folder", path: state.path, pattern: v.pattern || "*.csv", header: true }, queries));
    if (state.kind === "sqlite") {
      if (!v.table?.trim()) return setError(t("Enter the table's name."));
      return onDone(newQuery(v.table.trim(), { kind: "sqlite", path: state.path, table: v.table.trim() }, queries));
    }
    const port = Number(v.port) || 5432;
    for (const k of ["host", "database", "user", "table"]) if (!v[k]?.trim()) return setError(t("Fill in every field."));
    setBusy(true);
    try {
      if (v.password) await transport.rpc({ type: "setPassword", host: v.host!, port, database: v.database!, user: v.user!, password: v.password });
      onDone(newQuery(v.table!.trim(), { kind: "postgres", host: v.host!.trim(), port, database: v.database!.trim(), user: v.user!.trim(), schema: v.schema?.trim() || "public", table: v.table!.trim() }, queries));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const title = state.kind === "folder" ? t("Folder") : state.kind === "sqlite" ? t("SQLite database") : t("PostgreSQL database");
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone(null)}
      title={title}
      width={480}
      footer={
        <>
          <Button variant="ghost" onPress={() => onDone(null)}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy} onPress={ok}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <form className="qe-form" onSubmit={(e) => (e.preventDefault(), ok())}>
        {state.kind !== "postgres" && <p className="qe-path">{state.path}</p>}
        {state.kind === "folder" && (
          <>
            {input("pattern", t("Files to combine"))}
            <p className="qe-hint">{t("Every matching CSV file is read and combined by column name; a Source.Name column says which file each row came from.")}</p>
          </>
        )}
        {state.kind === "sqlite" && input("table", t("Table"))}
        {state.kind === "postgres" && (
          <>
            <div className="qe-row">
              {input("host", t("Server"))}
              {input("port", t("Port"), "number")}
            </div>
            {input("database", t("Database"))}
            <div className="qe-row">
              {input("schema", t("Schema"))}
              {input("table", t("Table"))}
            </div>
            <div className="qe-row">
              {input("user", t("User name"))}
              {input("password", t("Password"), "password")}
            </div>
            <p className="qe-hint">{t("The password is kept in your desktop keyring, never in the project.")}</p>
          </>
        )}
        {error && <p className="qe-error">{error}</p>}
      </form>
    </Dialog>
  );
}

export function SourceDialog({ state, queries, onDone }: { state: SourceDialogState; queries: Query[]; onDone(qs: Query[] | null): void }) {
  if (state.kind === "excel") return <ExcelNavigator path={state.path} queries={queries} onDone={onDone} />;
  return <SourceForm state={state} queries={queries} onDone={(q) => onDone(q ? [q] : null)} />;
}

/** Asks to download a DuckDB extension a source needs. */
export function ConsentDialog({ name, onDone }: { name: string; onDone(): void }) {
  const { t } = useT();
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone()}
      title={t("Download a database connector?")}
      width={480}
      footer={
        <>
          <Button variant="ghost" onPress={onDone}>
            {t("Not now")}
          </Button>
          <Button
            variant="primary"
            isDisabled={busy}
            onPress={async () => {
              setBusy(true);
              const ok = await useApp.getState().rpc({ type: "allowExtension", name }, t("Downloading…"));
              setBusy(false);
              if (ok) {
                clearQueryCache();
                toast({ kind: "success", title: t("Downloaded DuckDB's %1 extension", name) });
              }
              onDone();
            }}
          >
            {t("Download")}
          </Button>
        </>
      }
    >
      <p>{t("This source needs DuckDB's %1 extension. charted downloads it once from extensions.duckdb.org and keeps it for every project. Nothing about your data is sent.", name)}</p>
    </Dialog>
  );
}

/** Asks for a database password, saved to the keyring. */
export function PasswordDialog({ connection, onDone }: { connection: string; onDone(): void }) {
  const { t } = useT();
  const [password, setPassword] = useState("");
  const m = /^(.*)@(.*):(\d+)\/(.*)$/.exec(connection);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone()}
      title={t("Password")}
      width={420}
      footer={
        <>
          <Button variant="ghost" onPress={onDone}>
            {t("Cancel")}
          </Button>
          <Button
            variant="primary"
            isDisabled={!m}
            onPress={async () => {
              if (!m) return;
              if (await useApp.getState().rpc({ type: "setPassword", user: m[1]!, host: m[2]!, port: Number(m[3]), database: m[4]!, password })) clearQueryCache();
              onDone();
            }}
          >
            {t("Connect")}
          </Button>
        </>
      }
    >
      <label className="field">
        <span className="field-label">{t("Password for %1", connection)}</span>
        <input className="input" type="password" autoFocus value={password} onChange={(e) => setPassword(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
      </label>
    </Dialog>
  );
}
