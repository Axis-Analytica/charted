// Notices when a query's source file changes or disappears (polled every few seconds:
// works on every file system, including shared folders without change events) and
// offers Refresh, or Locate… for a missing file.
import { useEffect, useState } from "react";
import { transport } from "../bridge";
import type { ProjectSnapshot } from "../bridge/generated/ProjectSnapshot";
import type { SourceStatus } from "../bridge/generated/SourceStatus";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Banner } from "../kit/misc";
import { useApp } from "../state/app";
import { clearQueryCache } from "./data";
import { projectPath } from "./NewSource";

const POLL_MS = 3000;

export function SourceBanner({ project }: { project: ProjectSnapshot }) {
  const { t } = useT();
  const [status, setStatus] = useState<SourceStatus[]>([]);

  useEffect(() => {
    let live = true;
    const check = async () => {
      try {
        const buf = await transport.data({ type: "sourceStatus" });
        if (live) setStatus(JSON.parse(new TextDecoder().decode(buf)) as SourceStatus[]);
      } catch {
        // A project closing mid-request; the next check settles it.
      }
    };
    check();
    const timer = setInterval(check, POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [project.path, project.queries]);

  const missing = status.filter((s) => s.state === "missing");
  const changed = status.filter((s) => s.state === "changed");
  if (!missing.length && !changed.length) return null;

  const refresh = async () => {
    clearQueryCache();
    if (await useApp.getState().rpc({ type: "refresh" }, t("Refreshing…"))) setStatus([]);
  };
  const locate = async (s: SourceStatus) => {
    const q = project.queries.find((x) => x.id === s.query)?.definition;
    if (!q) return;
    const path = await transport.pick({ title: t("Locate %1", s.path), directory: q.source.kind === "folder" });
    if (!path) return;
    const queries = project.queries.map((x) => x.definition).map((x) => (x.id === q.id ? { ...x, source: { ...x.source, path: projectPath(project.path, path) } as typeof x.source } : x));
    clearQueryCache();
    await useApp.getState().rpc({ type: "applyQueries", queries }, t("Loading %1…", q.name));
  };

  if (missing.length)
    return (
      <Banner
        tone="warning"
        actions={
          <Button size="sm" variant="primary" onPress={() => locate(missing[0]!)}>
            {t("Locate…")}
          </Button>
        }
      >
        {t("The source of %1 is missing: %2", missing[0]!.name, missing[0]!.path)}
      </Banner>
    );
  return (
    <Banner
      tone="info"
      actions={
        <Button size="sm" variant="primary" onPress={refresh}>
          {t("Refresh")}
        </Button>
      }
    >
      {t("Source data changed: %1. Refresh to load it.", changed.map((s) => s.name).join(", "))}
    </Banner>
  );
}
