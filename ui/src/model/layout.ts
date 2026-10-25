// Lays out the model diagram with ELK in its own web worker, off the main thread.
// Filters flow from the one side to the many side, so dimension tables land left of
// the facts they filter.
import ELK from "elkjs/lib/elk-api.js";
import workerUrl from "elkjs/lib/elk-worker.min.js?url";

export interface LayoutRequest {
  nodes: { id: string; width: number; height: number }[];
  edges: { id: string; source: string; target: string }[];
}
export type LayoutResult = Record<string, { x: number; y: number }>;

let elk: InstanceType<typeof ELK> | null = null;

export async function layout({ nodes, edges }: LayoutRequest): Promise<LayoutResult> {
  elk ??= new ELK({ workerFactory: () => new Worker(workerUrl) });
  const graph = await elk.layout({
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.spacing.nodeNode": "48",
      "elk.layered.spacing.nodeNodeBetweenLayers": "140",
      "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
      "elk.padding": "[top=40,left=40,bottom=40,right=40]",
    },
    children: nodes,
    edges: edges.map((x) => ({ id: x.id, sources: [x.source], targets: [x.target] })),
  });
  const out: LayoutResult = {};
  for (const c of graph.children ?? []) out[c.id] = { x: Math.round(c.x ?? 0), y: Math.round(c.y ?? 0) };
  return out;
}
