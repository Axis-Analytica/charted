// Row-level security: the Manage roles dialog (each role's DAX filters per table and
// its members), View as (preview the report as one or more roles see it) and the
// banner shown while viewing.
import { Eye, Filter, Plus, ShieldCheck, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { create } from "zustand";
import { errorMessage, transport } from "../bridge";
import type { Change } from "../bridge/generated/Change";
import type { DaxCheck } from "../bridge/generated/DaxCheck";
import type { Model } from "../bridge/generated/Model";
import type { Role } from "../bridge/generated/Role";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { Menu } from "../kit/Menu";
import { useApp } from "../state/app";
import { dataJson } from "../state/dataview";
import { newId } from "../state/report";
import { useViewAs } from "../state/viewas";
import { DaxEditor } from "../dax/LazyDaxEditor";
import { useViewedRoles } from "./data";

/** Which roles dialog is open. */
export const useRolesDialog = create<{ open: "manage" | "viewAs" | null; set(open: "manage" | "viewAs" | null): void }>((set) => ({
  open: null,
  set: (open) => set({ open }),
}));

const roleList = (model: Model): Role[] => model.roles ?? [];

/** The Security menu in the report toolbar. */
export function RolesMenu({ model }: { model: Model }) {
  const { t } = useT();
  const open = useRolesDialog((s) => s.set);
  const viewing = useViewedRoles();
  return (
    <Menu
      label={t("Security")}
      trigger={<ShieldCheck size={15} />}
      items={[
        { id: "manage-roles", label: t("Manage roles…"), icon: <Filter size={14} />, run: () => open("manage") },
        { id: "view-as", label: t("View as…"), icon: <Eye size={14} />, disabled: !roleList(model).length, run: () => open("viewAs") },
        ...(viewing.length ? [{ id: "stop-viewing", label: t("Stop viewing"), run: () => useViewAs.getState().stop() }] : []),
      ]}
    />
  );
}

/** Shown above the canvas while the report is viewed as roles. */
export function ViewAsBanner({ model }: { model: Model }) {
  const { t } = useT();
  const viewing = useViewedRoles();
  const names = roleList(model)
    .filter((r) => viewing.includes(r.id))
    .map((r) => r.name);
  // Roles deleted while viewing (or an undo) end the preview.
  useEffect(() => {
    if (viewing.length && !names.length) useViewAs.getState().stop();
  }, [viewing.length, names.length]);
  if (!names.length) return null;
  return (
    <div className="view-as-bar" role="status" aria-label={t("View as")}>
      <Eye size={14} aria-hidden />
      <span className="view-as-roles">
        {t("Viewing as: %1", names.join(", "))}
      </span>
      <Button variant="ghost" size="sm" onPress={() => useViewAs.getState().stop()}>
        {t("Stop viewing")}
      </Button>
    </div>
  );
}

/** Power BI's "View as roles": pick the roles to preview the report as. */
export function ViewAsDialog({ model }: { model: Model }) {
  const { t } = useT();
  const open = useRolesDialog((s) => s.open === "viewAs");
  const setOpen = useRolesDialog((s) => s.set);
  const project = useApp((s) => s.state?.project?.path ?? null);
  const viewing = useViewedRoles();
  const [chosen, setChosen] = useState<string[]>([]);
  useEffect(() => {
    if (open) setChosen(viewing);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  const roles = roleList(model);
  const close = () => setOpen(null);
  const toggle = (id: string) => setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]));
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && close()}
      title={t("View as roles")}
      width={420}
      footer={
        <>
          <Button variant="ghost" onPress={close}>
            {t("Cancel")}
          </Button>
          <Button
            variant="primary"
            onPress={() => {
              useViewAs.getState().set(project, roles.filter((r) => chosen.includes(r.id)).map((r) => r.id));
              close();
            }}
          >
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="view-as-list" role="group" aria-label={t("Roles")}>
        <label className="check">
          <input type="checkbox" checked={!chosen.length} onChange={() => setChosen([])} />
          {t("None")}
        </label>
        {roles.map((r) => (
          <label key={r.id} className="check">
            <input type="checkbox" checked={chosen.includes(r.id)} onChange={() => toggle(r.id)} />
            {r.name}
          </label>
        ))}
      </div>
      <p className="field-hint">{t("Several roles show the rows any of them can see.")}</p>
    </Dialog>
  );
}

/** A unique "Role 2"-style name. */
function nextName(roles: Role[], base: string): string {
  const taken = new Set(roles.map((r) => r.name.toLowerCase()));
  if (!taken.has(base.toLowerCase())) return base;
  for (let i = 2; ; i++) if (!taken.has(`${base} ${i}`.toLowerCase())) return `${base} ${i}`;
}

/** A role with its optional parts filled in, to compare. */
const normal = (r: Role) => ({ ...r, description: r.description ?? null, members: r.members ?? [], extras: r.extras ?? {} });

/** The changes that turn the model's roles into `draft`, in one undo step. */
function roleChanges(before: Role[], draft: Role[]): Change[] {
  const changes: Change[] = before.filter((r) => !draft.some((d) => d.id === r.id)).map((r) => ({ op: "removeRole", role: r.id }));
  draft.forEach((r, index) => {
    const old = before.find((b) => b.id === r.id);
    if (!old) changes.push({ op: "addRole", role: r, index });
    else if (JSON.stringify(normal(old)) !== JSON.stringify(normal(r))) changes.push({ op: "updateRole", role: r });
  });
  return changes;
}

/** Manage roles: add, rename and delete roles, filter tables with DAX, list members. */
export function ManageRolesDialog({ model }: { model: Model }) {
  const { t } = useT();
  const open = useRolesDialog((s) => s.open === "manage");
  const setOpen = useRolesDialog((s) => s.set);
  const [draft, setDraft] = useState<Role[]>([]);
  const [roleId, setRoleId] = useState<string | null>(null);
  const [tableId, setTableId] = useState<string | null>(null);
  const [check, setCheck] = useState<DaxCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!open) return;
    const roles = roleList(model);
    setDraft(roles);
    setRoleId(roles[0]?.id ?? null);
    setTableId(roles[0]?.filters[0]?.table ?? model.tables[0]?.id ?? null);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  const role = draft.find((r) => r.id === roleId) ?? null;
  const table = model.tables.find((x) => x.id === tableId) ?? null;
  const expression = role?.filters.find((f) => f.table === tableId)?.expression ?? "";
  const update = (f: (r: Role) => Role) => role && setDraft((d) => d.map((r) => (r.id === role.id ? f(r) : r)));
  const setExpression = (text: string) =>
    update((r) => {
      const others = r.filters.filter((f) => f.table !== tableId);
      if (!text.trim() || !tableId) return { ...r, filters: others };
      const i = r.filters.findIndex((f) => f.table === tableId);
      const filters = [...r.filters];
      if (i >= 0) filters[i] = { table: tableId, expression: text };
      else filters.push({ table: tableId, expression: text });
      return { ...r, filters };
    });

  // The filter being edited, checked as a condition on its table's rows.
  useEffect(() => {
    setCheck(null);
    if (!open || !tableId || !expression.trim()) return;
    let live = true;
    const timer = setTimeout(() => {
      dataJson<DaxCheck>({ type: "checkRoleFilter", table: tableId, expression })
        .then((c) => live && setCheck(c))
        .catch(() => {});
    }, 200);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [open, tableId, expression]);
  const problem = check?.diagnostics[0];

  const nameProblem = useMemo(() => {
    for (const r of draft) {
      if (!r.name.trim()) return t("Every role needs a name.");
      if (draft.some((o) => o.id !== r.id && o.name.trim().toLowerCase() === r.name.trim().toLowerCase())) return t("There are two roles named \"%1\".", r.name.trim());
    }
    return null;
  }, [draft, t]);

  const addRole = () => {
    const r: Role = { id: newId(), name: nextName(draft, t("New role")), description: null, filters: [], members: [] };
    setDraft((d) => [...d, r]);
    setRoleId(r.id);
    setTableId(model.tables[0]?.id ?? null);
  };
  const removeRole = () => {
    if (!role) return;
    const rest = draft.filter((r) => r.id !== role.id);
    setDraft(rest);
    setRoleId(rest[0]?.id ?? null);
  };
  const close = () => setOpen(null);
  const save = async () => {
    const cleaned = draft.map((r) => ({ ...r, name: r.name.trim(), members: (r.members ?? []).map((m) => m.trim()).filter(Boolean) }));
    const changes = roleChanges(roleList(model), cleaned);
    if (!changes.length) return close();
    setBusy(true);
    setError(null);
    try {
      const state = await transport.rpc({ type: "commit", label: "Edit roles", changes });
      useApp.setState({ state });
      close();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && close()}
      title={t("Manage roles")}
      width={860}
      modal
      footer={
        <>
          {(error ?? nameProblem) && (
            <span className="roles-error" role="alert">
              {error ?? nameProblem}
            </span>
          )}
          <span className="dialog-spacer" />
          <Button variant="ghost" onPress={close}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" isDisabled={busy || !!nameProblem} onPress={save}>
            {t("Save")}
          </Button>
        </>
      }
    >
      <div className="roles-dialog">
        <section className="roles-col" aria-label={t("Roles")}>
          <div className="roles-col-head">
            <span>{t("Roles")}</span>
            <IconButton label={t("New role")} icon={<Plus size={14} />} onPress={addRole} />
          </div>
          <ul className="roles-items" role="listbox" aria-label={t("Roles")}>
            {draft.map((r) => (
              <li key={r.id} role="option" aria-selected={r.id === roleId} className={r.id === roleId ? "is-on" : ""} onClick={() => setRoleId(r.id)}>
                {r.name || t("(no name)")}
              </li>
            ))}
          </ul>
          {!draft.length && <p className="field-hint">{t("No roles yet. A role limits the rows its members see.")}</p>}
        </section>
        {role ? (
          <>
            <section className="roles-col" aria-label={t("Tables")}>
              <div className="roles-col-head">
                <span>{t("Tables")}</span>
              </div>
              <ul className="roles-items" role="listbox" aria-label={t("Tables")}>
                {model.tables.map((x) => {
                  const filtered = role.filters.some((f) => f.table === x.id);
                  return (
                    <li key={x.id} role="option" aria-selected={x.id === tableId} className={x.id === tableId ? "is-on" : ""} onClick={() => setTableId(x.id)}>
                      <span>{x.name}</span>
                      {filtered && <Filter size={12} aria-label={t("Filtered")} />}
                    </li>
                  );
                })}
              </ul>
            </section>
            <section className="roles-edit">
              <label className="field">
                <span className="field-label">{t("Role name")}</span>
                <input className="input" value={role.name} onChange={(e) => update((r) => ({ ...r, name: e.target.value }))} />
              </label>
              {table && (
                <div className="field">
                  <span className="field-label">{t("Filter the rows of %1 (a DAX condition, true for the rows the role sees)", table.name)}</span>
                  <div className={`roles-dax ${problem ? "has-error" : ""}`}>
                    <DaxEditor
                      key={`${role.id}\n${table.id}`}
                      label={t("Role filter")}
                      model={model}
                      home={table.id}
                      value={expression}
                      onChange={setExpression}
                      onCommit={() => {}}
                      onCancel={() => {}}
                      maxLines={6}
                      extra={problem ? { message: problem.message, start: problem.span.start, end: problem.span.end } : null}
                    />
                  </div>
                  {problem ? (
                    <span className="roles-error" role="alert">
                      {problem.message}
                    </span>
                  ) : (
                    <span className="field-hint">{expression.trim() ? t("Rows where this is false are hidden from the role, and so are the rows that relate to them.") : t("No filter: the role sees every row of this table.")}</span>
                  )}
                  {expression.trim() && (
                    <div>
                      <Button variant="ghost" size="sm" icon={<Trash2 size={14} />} onPress={() => setExpression("")}>
                        {t("Remove filter")}
                      </Button>
                    </div>
                  )}
                </div>
              )}
              <label className="field">
                <span className="field-label">{t("Members (one per line; kept for Power BI)")}</span>
                <textarea
                  className="input"
                  rows={3}
                  value={(role.members ?? []).join("\n")}
                  onChange={(e) => update((r) => ({ ...r, members: e.target.value.split("\n") }))}
                />
              </label>
              <div>
                <Button variant="danger" size="sm" icon={<Trash2 size={14} />} onPress={removeRole}>
                  {t("Delete role")}
                </Button>
              </div>
            </section>
          </>
        ) : (
          <div className="roles-edit" />
        )}
      </div>
    </Dialog>
  );
}
