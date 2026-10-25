// The query editor's requests to the core: a step's preview rows, columns (with SQL and
// M) and profile, cached by everything they depend on.
import { tableFromIPC, type Vector } from "apache-arrow";
import { useEffect, useState } from "react";
import { type ApiError, isApiError, transport } from "../bridge";
import type { Analysis } from "../bridge/generated/Analysis";
import type { Query } from "../bridge/generated/Query";
import type { QueryInfo } from "../bridge/generated/QueryInfo";
import type { QueryProfile } from "../bridge/generated/QueryProfile";

const decoder = new TextDecoder();
const parse = <T,>(buf: ArrayBuffer): T => JSON.parse(decoder.decode(buf)) as T;

/** Queries the request depends on: the query and those it merges or appends. */
function related(query: Query, queries: Query[]): Query[] {
  const out: Query[] = [];
  const visit = (q: Query) => {
    if (out.some((x) => x.id === q.id)) return;
    out.push(q);
    for (const s of q.steps) {
      const refs = s.op === "mergeQueries" ? [s.query] : s.op === "appendQueries" ? s.queries : [];
      for (const r of refs) {
        const o = queries.find((x) => x.id === r);
        if (o) visit(o);
      }
    }
  };
  visit(query);
  return out;
}

export async function queryInfo(query: Query, queries: Query[], upto: number): Promise<QueryInfo> {
  return parse(await transport.data({ type: "queryInfo", query, queries: related(query, queries), upto }));
}

export async function analyze(query: Query, queries: Query[], upto: number, analysis: Analysis): Promise<unknown> {
  return parse(await transport.data({ type: "queryAnalyze", query, queries: related(query, queries), upto, analysis }));
}

export interface Preview {
  rows: number;
  names: string[];
  columns: Vector[];
}

const MAX = 80;
const cache = new Map<string, Promise<unknown>>();
function cached<T>(key: string, load: () => Promise<T>): Promise<T> {
  let p = cache.get(key) as Promise<T> | undefined;
  if (!p) {
    p = load();
    cache.set(key, p);
    p.catch(() => cache.delete(key));
    while (cache.size > MAX) cache.delete(cache.keys().next().value!);
  }
  return p;
}

/** Forgets everything cached (sources changed on disk). */
export function clearQueryCache() {
  cache.clear();
}

interface State<T> {
  data: T | null;
  error: ApiError | null;
  loading: boolean;
}

function useCached<T>(key: string | null, load: () => Promise<T>): State<T> {
  const [s, setS] = useState<State<T> & { key: string | null }>({ key: null, data: null, error: null, loading: false });
  useEffect(() => {
    if (!key) {
      setS({ key, data: null, error: null, loading: false });
      return;
    }
    let live = true;
    setS((o) => ({ ...o, key, loading: true }));
    cached(key, load).then(
      (data) => live && setS({ key, data, error: null, loading: false }),
      (e) => live && setS({ key, data: null, error: isApiError(e) ? e : { code: "error", message: String(e) }, loading: false }),
    );
    return () => {
      live = false;
    };
    // The key captures everything the request depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return s;
}

const keyOf = (kind: string, query: Query, queries: Query[], upto: number, extra = "") => JSON.stringify([kind, related(query, queries), query.id, upto, extra]);

export function useQueryInfo(query: Query | null, queries: Query[], upto: number) {
  return useCached(query ? keyOf("info", query, queries, upto) : null, () => queryInfo(query!, queries, upto));
}

export function usePreview(query: Query | null, queries: Query[], upto: number) {
  return useCached<Preview>(query ? keyOf("preview", query, queries, upto) : null, async () => {
    const buf = await transport.data({ type: "queryPreview", query: query!, queries: related(query!, queries), upto, limit: 1000 });
    const table = tableFromIPC(new Uint8Array(buf));
    return {
      rows: table.numRows,
      names: table.schema.fields.map((f) => f.name),
      columns: table.schema.fields.map((_, i) => table.getChildAt(i)!),
    };
  });
}

export function useProfile(query: Query | null, queries: Query[], upto: number, full: boolean) {
  return useCached<QueryProfile>(query ? keyOf("profile", query, queries, upto, String(full)) : null, async () =>
    parse(await transport.data({ type: "queryProfile", query: query!, queries: related(query!, queries), upto, full })),
  );
}
