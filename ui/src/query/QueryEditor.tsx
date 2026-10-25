// Transform data (Power Query's editor): the queries, a step's preview with column
// profiles, the applied steps, transforms from the toolbar or a column's menu, and Close
// & Apply, which loads the changed queries and updates the model.
import { Check, ChevronDown, Copy, Database, FileSpreadsheet, FileText, FolderOpen, GripVertical, Pencil, Plus, Redo2, Settings2, Trash2, Undo2, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { errorMessage, transport } from "../bridge";
import type { DataType } from "../bridge/generated/DataType";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import type { Query } from "../bridge/generated/Query";
import type { Step } from "../bridge/generated/Step";
import { useShortcut } from "../app/shortcuts";
import { msg, useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { ConfirmDialog } from "../kit/Dialog";
import { Menu, type MenuEntry } from "../kit/Menu";
import { Banner, Spinner } from "../kit/misc";
import { useApp } from "../state/app";
import { useReport } from "../state/report";
import { toast } from "../state/toasts";
import { analyze, useProfile, usePreview, useQueryInfo } from "./data";
import { ConsentDialog, newQuery, PasswordDialog, pickSource, projectPath, SourceDialog, type SourceDialogState, type SourceKind } from "./NewSource";
import { PreviewTable } from "./PreviewTable";
import { insertStep, isDirty, moveStep, removeStep, uniqueQueryName, uptoOf, useQueryDraft } from "./state";
import { StepDialog } from "./StepDialog";
import { type BuildContext, stepDisplayNames, type Transform, TRANSFORMS, transformById, transformFor } from "./transforms";
import "./query.css";

const STEP_MIME = "application/x-charted-step";

const GROUPS = ["Manage columns", "Reduce rows", "Transform", "Add column", "Combine"];

const SOURCES: { kind: SourceKind; label: string; icon: React.ReactNode }[] = [
  { kind: "csv", label: "Text/CSV", icon: <FileText size={14} /> },
  { kind: "excel", label: "Excel workbook", icon: <FileSpreadsheet size={14} /> },
  { kind: "json", label: "JSON", icon: <FileText size={14} /> },
  { kind: "parquet", label: "Parquet", icon: <FileText size={14} /> },
  { kind: "folder", label: "Folder", icon: <FolderOpen size={14} /> },
  { kind: "sqlite", label: "SQLite database", icon: <Database size={14} /> },
  { kind: "postgres", label: "PostgreSQL database", icon: <Database size={14} /> },
];

export function QueryEditor({ project }: { project: ProjectSnapshot }) {
  const { t } = useT();
  const draft = useQueryDraft();
  const { queries, selected, upto: uptoMap } = draft;
  const defs = useMemo(() => project.queries.map((q) => q.definition), [project.queries]);
  const [dialog, setDialog] = useState<{ transform: Transform; step: Step | null; column: string | null; index: number | null } | null>(null);
  const [sourceDialog, setSourceDialog] = useState<SourceDialogState | null>(null);
  const [consent, setConsent] = useState<string | null>(null);
  const [password, setPassword] = useState<string | null>(null);
  const [discard, setDiscard] = useState(false);
  const [renaming, setRenaming] = useState<number | null>(null);
  const [code, setCode] = useState<"sql" | "m" | null>("sql");
  const [full, setFull] = useState(false);
  const [drag, setDrag] = useState<number | null>(null);
  const dirty = isDirty(draft.base, queries);

  // The draft starts from the project's queries, and follows them while untouched.
  useEffect(() => {
    const s = useQueryDraft.getState();
    if (!isDirty(s.base, s.queries) && JSON.stringify(s.base) !== JSON.stringify(defs)) s.open(defs);
  }, [defs]);

  const query = queries.find((q) => q.id === selected) ?? queries[0] ?? null;
  const upto = query ? uptoOf(query, uptoMap) : 0;
  const info = useQueryInfo(query, queries, upto);
  const preview = usePreview(query, queries, upto);
  const profile = useProfile(query, queries, upto, full);
  const error = preview.error ?? info.error;
  const columns = info.data?.columns.map((c) => c.name) ?? preview.data?.names ?? [];

  useEffect(() => {
    if (error?.code === "consent" && error.detail) setConsent(error.detail);
    if (error?.code === "password" && error.detail) setPassword(error.detail);
  }, [error?.code, error?.detail]);

  const setQuery = (label: string, q: Query, select?: number) =>
    draft.change(
      label,
      queries.map((x) => (x.id === q.id ? q : x)),
      { query: q.id, upto: select },
    );
  const ctx = (): BuildContext => ({
    columns,
    query: query!,
    queries,
    analyze: (a) => analyze(query!, queries, upto, a),
  });
  /** Adds a step after the selected one (Power Query inserts there too). */
  const addStep = (s: Step, label = "Add step") => {
    if (!query) return;
    const at = upto;
    setQuery(label, insertStep(query, at, s), at + 1 === query.steps.length + 1 ? undefined : at + 1);
  };
  const run = async (tr: Transform, column: string | null) => {
    if (!query) return;
    if (tr.direct) {
      try {
        addStep(await tr.build({}, ctx()), tr.label);
      } catch (e) {
        toast({ kind: "error", title: errorMessage(e) });
      }
      return;
    }
    setDialog({ transform: tr, step: null, column, index: null });
  };

  const columnMenu = (c: string): (MenuEntry | null)[] => {
    const direct = (id: string, label: string, s: Step) => ({ id, label: t(label), run: () => addStep(s, label) });
    const open = (id: string) => {
      const tr = transformById(id)!;
      return { id, label: `${t(tr.label)}…`, run: () => run(tr, c) };
    };
    return [
      direct("remove", msg("Remove"), { op: "removeColumns", columns: [c] }),
      direct("removeOthers", msg("Remove other columns"), { op: "selectColumns", columns: [c] }),
      open("renameColumn"),
      null,
      direct("sortAsc", msg("Sort ascending"), { op: "sortRows", columns: [[c, false]] }),
      direct("sortDesc", msg("Sort descending"), { op: "sortRows", columns: [[c, true]] }),
      open("filterRows"),
      direct("removeDupes", msg("Remove duplicates"), { op: "removeDuplicates", columns: [c] }),
      null,
      open("replaceValues"),
      open("splitColumn"),
      open("formatText"),
      open("extractText"),
      direct("fillDown", msg("Fill down"), { op: "fillDown", columns: [c] }),
      null,
      direct("unpivot", msg("Unpivot columns"), { op: "unpivot", columns: [c], attribute: "Attribute", value: "Value" }),
      direct("unpivotOthers", msg("Unpivot other columns"), { op: "unpivot", columns: columns.filter((x) => x !== c), attribute: "Attribute", value: "Value" }),
      open("pivot"),
      open("groupBy"),
    ];
  };

  const changeType = (column: string, type: DataType) => {
    if (!query) return;
    // Changing a type right after a type change folds into it, as Power Query does.
    const prev = upto > 0 ? query.steps[upto - 1] : undefined;
    if (prev?.op === "changeTypes") {
      const cols = prev.columns.filter((x) => x[0] !== column);
      cols.push([column, type]);
      const steps = [...query.steps];
      steps[upto - 1] = { ...prev, columns: cols };
      setQuery("Change type", { ...query, steps }, upto === query.steps.length ? undefined : upto);
    } else addStep({ op: "changeTypes", columns: [[column, type]] }, "Change type");
  };

  const getData = async (kind: SourceKind) => {
    const r = await pickSource(kind, project.path, queries, t);
    if (r?.query) draft.change("New source", [...queries, r.query], { query: r.query.id });
    if (r?.dialog) setSourceDialog(r.dialog);
  };

  const apply = async () => {
    const names = queries.filter((q) => !q.noLoad).map((q) => q.name);
    const ok = await useApp.getState().rpc({ type: "applyQueries", queries }, t("Loading %1…", names.slice(0, 3).join(", ") + (names.length > 3 ? "…" : "")));
    if (ok) {
      const latest = useApp.getState().state?.project?.queries.map((q) => q.definition) ?? queries;
      useQueryDraft.getState().open(latest);
      useReport.getState().set({ view: "report" });
    }
  };

  useShortcut(
    { id: "query-undo", keys: "Ctrl+Z", label: "Undo in the query editor", group: "Edit", run: () => useQueryDraft.getState().undo(), enabled: () => useReport.getState().view === "query" && useQueryDraft.getState().past.length > 0 },
    { id: "query-redo", keys: "Ctrl+Shift+Z", label: "Redo in the query editor", group: "Edit", run: () => useQueryDraft.getState().redo(), enabled: () => useReport.getState().view === "query" && useQueryDraft.getState().future.length > 0 },
  );

  const names = query ? stepDisplayNames(query) : [];

  return (
    <div className="qe">
      <div className="qe-ribbon" role="toolbar" aria-label={t("Transform data")}>
        <Menu label={t("New source")} trigger={<span className="qe-ribbon-btn"><Plus size={14} /> {t("New source")} <ChevronDown size={12} /></span>} items={SOURCES.map((s) => ({ id: s.kind, label: t(s.label), icon: s.icon, run: () => getData(s.kind) }))} />
        <span className="topbar-sep" />
        {GROUPS.map((g) => (
          <Menu
            key={g}
            label={t(g)}
            trigger={<span className="qe-ribbon-btn">{t(g)} <ChevronDown size={12} /></span>}
            items={TRANSFORMS.filter((x) => x.group === g).map((x) => ({ id: x.id, label: x.direct ? t(x.label) : `${t(x.label)}…`, disabled: !query || (!columns.length && x.id !== "promoteHeaders"), run: () => run(x, null) }))}
          />
        ))}
        <span className="topbar-sep" />
        <IconButton label={t("Undo in the query editor")} icon={<Undo2 size={15} />} isDisabled={!draft.past.length} onPress={draft.undo} />
        <IconButton label={t("Redo in the query editor")} icon={<Redo2 size={15} />} isDisabled={!draft.future.length} onPress={draft.redo} />
        <span className="status-fill" />
        <Button variant="ghost" size="sm" isDisabled={!dirty} onPress={() => setDiscard(true)}>
          {t("Discard changes")}
        </Button>
        <Button variant="primary" size="sm" icon={<Check size={14} />} isDisabled={!dirty} onPress={apply}>
          {t("Close & Apply")}
        </Button>
      </div>
      <div className="qe-body">
        <aside className="qe-queries" aria-label={t("Queries")}>
          <div className="pane-head">
            <span>{t("Queries [%1]", queries.length)}</span>
          </div>
          <ul role="listbox" aria-label={t("Queries")}>
            {queries.map((q) => (
              <li key={q.id} role="option" aria-selected={q.id === query?.id} className={`qe-query ${q.id === query?.id ? "is-on" : ""} ${q.noLoad ? "is-noload" : ""}`} onClick={() => draft.select(q.id)}>
                <FileText size={13} />
                <span className="qe-query-name">{q.name}</span>
                <Menu
                  label={t("Options for %1", q.name)}
                  trigger={<ChevronDown size={12} />}
                  items={[
                    {
                      id: "duplicate",
                      label: t("Duplicate"),
                      icon: <Copy size={13} />,
                      run: () => {
                        const copy = newQuery(q.name, q.source, queries);
                        draft.change("Duplicate query", [...queries, { ...copy, steps: q.steps, stepNames: q.stepNames }], { query: copy.id });
                      },
                    },
                    {
                      id: "load",
                      label: q.noLoad ? t("Enable load") : t("Disable load (only feeds other queries)"),
                      run: () => draft.change("Change load", queries.map((x) => (x.id === q.id ? { ...x, noLoad: !x.noLoad } : x))),
                    },
                    {
                      id: "delete",
                      label: t("Delete"),
                      icon: <Trash2 size={13} />,
                      disabled: queries.some((o) => o.id !== q.id && o.steps.some((s) => (s.op === "mergeQueries" && s.query === q.id) || (s.op === "appendQueries" && s.queries.includes(q.id)))),
                      run: () => draft.change("Delete query", queries.filter((x) => x.id !== q.id), { query: queries.find((x) => x.id !== q.id)?.id ?? "" }),
                    },
                  ]}
                />
              </li>
            ))}
          </ul>
        </aside>
        <main className="qe-center">
          {!query ? (
            <div className="main-empty">{t("No queries yet. Get data with New source.")}</div>
          ) : (
            <>
              {error && (
                <Banner
                  tone="danger"
                  actions={
                    error.code === "consent" && error.detail ? (
                      <Button size="sm" onPress={() => setConsent(error.detail!)}>
                        {t("Download…")}
                      </Button>
                    ) : error.code === "password" && error.detail ? (
                      <Button size="sm" onPress={() => setPassword(error.detail!)}>
                        {t("Enter password…")}
                      </Button>
                    ) : upto > 0 ? (
                      <Button size="sm" variant="ghost" onPress={() => draft.selectStep(query.id, upto - 1)}>
                        {t("Go to the previous step")}
                      </Button>
                    ) : null
                  }
                >
                  {error.message}
                </Banner>
              )}
              <div className="qe-code">
                <div className="qe-code-tabs" role="tablist">
                  <button type="button" role="tab" aria-selected={code === "sql"} onClick={() => setCode(code === "sql" ? null : "sql")}>
                    SQL
                  </button>
                  <button type="button" role="tab" aria-selected={code === "m"} onClick={() => setCode(code === "m" ? null : "m")}>
                    M
                  </button>
                  <span className="qe-code-note">{code === "sql" ? t("What DuckDB runs for this step (read only)") : code === "m" ? t("The query in Power Query's M, as exported to Power BI") : ""}</span>
                </div>
                {code && info.data && (
                  <pre className="qe-code-text" aria-label={code === "sql" ? t("SQL") : t("M")}>
                    {code === "sql" ? info.data.sql : info.data.m}
                  </pre>
                )}
              </div>
              <div className="qe-grid">
                {preview.data ? (
                  <PreviewTable preview={preview.data} columns={info.data?.columns} profile={profile.data?.columns ?? null} menu={columnMenu} onType={changeType} />
                ) : preview.loading ? (
                  <div className="main-empty">
                    <Spinner />
                  </div>
                ) : null}
              </div>
              <div className="qe-status">
                <span>{t("%1 columns, %2 rows", columns.length, preview.data ? (preview.data.rows >= 1000 ? "1,000+" : preview.data.rows) : "…")}</span>
                <span className="status-fill" />
                <button type="button" className="link-btn" onClick={() => setFull((f) => !f)}>
                  {full ? t("Column profiling based on the entire data set") : t("Column profiling based on the first 1,000 rows")}
                </button>
                {(preview.loading || profile.loading) && <Spinner size={12} />}
              </div>
            </>
          )}
        </main>
        {query && (
          <aside className="qe-settings" aria-label={t("Query settings")}>
            <div className="pane-head">
              <span>{t("Query settings")}</span>
            </div>
            <div className="qe-settings-body">
              <label className="field">
                <span className="field-label">{t("Name")}</span>
                <input
                  className="input"
                  key={query.id + query.name}
                  defaultValue={query.name}
                  aria-label={t("Query name")}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                  }}
                  onBlur={(e) => {
                    const n = e.target.value.trim();
                    if (n && n !== query.name) setQuery("Rename query", { ...query, name: uniqueQueryName(n, queries.filter((x) => x.id !== query.id)) }, upto === query.steps.length ? undefined : upto);
                  }}
                />
              </label>
              <label className="check">
                <input type="checkbox" checked={!query.noLoad} onChange={(e) => setQuery("Change load", { ...query, noLoad: !e.target.checked })} />
                {t("Load to report")}
              </label>
              <div className="field-label qe-steps-head">{t("Applied steps")}</div>
              <ol className="qe-steps" aria-label={t("Applied steps")}>
                {names.map((name, i) => {
                  const s = i > 0 ? query.steps[i - 1] : null;
                  const tr = s ? transformFor(s) : null;
                  return (
                    <li
                      key={i}
                      className={`qe-step ${i === upto ? "is-on" : ""} ${i > upto ? "is-after" : ""} ${drag !== null && drag !== i ? "is-droppable" : ""}`}
                      aria-current={i === upto ? "step" : undefined}
                      draggable={i > 0 && renaming !== i}
                      onDragStart={(e) => {
                        e.dataTransfer.setData(STEP_MIME, String(i));
                        e.dataTransfer.effectAllowed = "move";
                        setDrag(i);
                      }}
                      onDragEnd={() => setDrag(null)}
                      onDragOver={(e) => {
                        if (i > 0 && e.dataTransfer.types.includes(STEP_MIME)) e.preventDefault();
                      }}
                      onDrop={(e) => {
                        const from = Number(e.dataTransfer.getData(STEP_MIME));
                        if (from > 0 && i > 0 && from !== i) setQuery("Move step", moveStep(query, from - 1, i - 1), undefined);
                        setDrag(null);
                      }}
                      tabIndex={0}
                      onClick={() => draft.selectStep(query.id, i)}
                      onDoubleClick={() => i > 0 && setRenaming(i)}
                      onKeyDown={(e) => {
                        if (renaming === i) return;
                        if (e.key === "Enter") draft.selectStep(query.id, i);
                        if (e.key === "F2" && i > 0) setRenaming(i);
                        // Alt+↑/↓ moves the step (the source stays first).
                        if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && i > 0) {
                          e.preventDefault();
                          const to = e.key === "ArrowUp" ? i - 1 : i + 1;
                          if (to >= 1 && to <= query.steps.length) {
                            setQuery("Move step", moveStep(query, i - 1, to - 1), undefined);
                            requestAnimationFrame(() => (document.querySelectorAll<HTMLElement>(".qe-step")[to])?.focus());
                          }
                        }
                      }}
                    >
                      {i > 0 ? <GripVertical size={12} className="qe-grip" /> : <span className="qe-grip" />}
                      {renaming === i ? (
                        <input
                          className="input qe-step-input"
                          autoFocus
                          defaultValue={name}
                          aria-label={t("Step name")}
                          onClick={(e) => e.stopPropagation()}
                          onKeyDown={(e) => {
                            e.stopPropagation();
                            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                            if (e.key === "Escape") setRenaming(null);
                          }}
                          onBlur={(e) => {
                            setRenaming(null);
                            const n = e.target.value.trim();
                            if (n && n !== name) setQuery("Rename step", { ...query, stepNames: { ...(query.stepNames ?? {}), [i - 1]: n } }, upto === query.steps.length ? undefined : upto);
                          }}
                        />
                      ) : (
                        <span className="qe-step-name">{name}</span>
                      )}
                      {i > 0 && renaming !== i && (
                        <span className="qe-step-tools">
                          <button type="button" aria-label={t("Rename %1", name)} title={t("Rename")} onClick={(e) => (e.stopPropagation(), setRenaming(i))}>
                            <Pencil size={12} />
                          </button>
                          {tr && tr.fields.length > 0 && (
                            <button type="button" aria-label={t("Edit %1", name)} title={t("Edit settings")} onClick={(e) => (e.stopPropagation(), setDialog({ transform: tr, step: s ?? null, column: null, index: i - 1 }))}>
                              <Settings2 size={12} />
                            </button>
                          )}
                          <button
                            type="button"
                            aria-label={t("Delete %1", name)}
                            title={t("Delete")}
                            onClick={(e) => {
                              e.stopPropagation();
                              setQuery("Delete step", removeStep(query, i - 1), upto >= i && upto < query.steps.length ? upto - 1 : undefined);
                            }}
                          >
                            <X size={12} />
                          </button>
                        </span>
                      )}
                      {i === 0 && query.source.kind !== "postgres" && (
                        <span className="qe-step-tools">
                          <button
                            type="button"
                            aria-label={t("Change the source")}
                            title={t("Change the source")}
                            onClick={async (e) => {
                              e.stopPropagation();
                              const path = await transport.pick({ title: t("Locate the source"), directory: query.source.kind === "folder" });
                              if (path) setQuery("Change source", { ...query, source: { ...query.source, path: projectPath(project.path, path) } as Query["source"] });
                            }}
                          >
                            <Settings2 size={12} />
                          </button>
                        </span>
                      )}
                    </li>
                  );
                })}
              </ol>
              {upto < query.steps.length && <p className="qe-hint">{t("New steps go after the selected one.")}</p>}
            </div>
          </aside>
        )}
      </div>
      {dialog && query && (
        <StepDialog
          transform={dialog.transform}
          step={dialog.step}
          column={dialog.column}
          ctx={ctx()}
          onDone={(s) => {
            const d = dialog;
            setDialog(null);
            if (!s) return;
            if (d.index !== null) {
              const steps = [...query.steps];
              steps[d.index] = s;
              setQuery("Edit step", { ...query, steps }, upto === query.steps.length ? undefined : upto);
            } else addStep(s, d.transform.label);
          }}
        />
      )}
      {sourceDialog && (
        <SourceDialog
          state={sourceDialog}
          queries={queries}
          onDone={(qs) => {
            setSourceDialog(null);
            if (qs?.length) draft.change("New source", [...queries, ...qs], { query: qs[0]!.id });
          }}
        />
      )}
      {consent && <ConsentDialog name={consent} onDone={() => setConsent(null)} />}
      {password && <PasswordDialog connection={password} onDone={() => setPassword(null)} />}
      <ConfirmDialog
        open={discard}
        title={t("Discard your changes to the queries?")}
        message={t("The queries go back to how they were when you last applied them.")}
        confirm={t("Discard")}
        cancel={t("Cancel")}
        danger
        onConfirm={() => {
          setDiscard(false);
          draft.open(defs);
        }}
        onCancel={() => setDiscard(false)}
      />
    </div>
  );
}

