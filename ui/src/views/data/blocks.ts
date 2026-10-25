// Rows of a view arrive in blocks of 200 from the core, as Arrow. Blocks live in a small
// LRU cache; when the user scrolls faster than blocks load, requests for rows that have
// already scrolled away are dropped before they are sent, and late replies are ignored.
import { tableFromIPC, type Table as ArrowTable, type Vector } from "apache-arrow";
import { transport } from "../../bridge";
import type { ViewSpec } from "../../bridge/generated/ViewSpec";

export const BLOCK_ROWS = 200;
const MAX_BLOCKS = 64;
const MAX_IN_FLIGHT = 3;

interface Block {
  rows: number;
  columns: Map<string, Vector>;
}

export class BlockCache {
  private blocks = new Map<number, Block>();
  private inFlight = new Set<number>();
  private wanted: number[] = [];
  private closed = false;
  error: string | null = null;

  constructor(
    private view: ViewSpec,
    private onBlock: (first: number, last: number) => void,
    private onError: (message: string) => void,
  ) {}

  /** Stops delivering replies. `open` undoes it (React's development double-mount). */
  close() {
    this.closed = true;
  }

  open() {
    this.closed = false;
  }

  /** The Arrow vector and index for a cell, or undefined while its block loads. */
  cell(column: string, row: number): { vector: Vector; index: number } | undefined {
    const b = this.blocks.get(Math.floor(row / BLOCK_ROWS));
    const vector = b?.columns.get(column);
    return vector ? { vector, index: row % BLOCK_ROWS } : undefined;
  }

  has(row: number): boolean {
    return this.blocks.has(Math.floor(row / BLOCK_ROWS));
  }

  /** Asks for the rows in view (plus one block either side). Blocks that are no longer
   * wanted and not yet requested are forgotten. */
  want(firstRow: number, lastRow: number, totalRows: number) {
    const first = Math.max(0, Math.floor(firstRow / BLOCK_ROWS) - 1);
    const last = Math.min(Math.floor(Math.max(0, totalRows - 1) / BLOCK_ROWS), Math.floor(lastRow / BLOCK_ROWS) + 1);
    const visible: number[] = [];
    const around: number[] = [];
    for (let b = first; b <= last; b++) {
      const hit = this.blocks.get(b);
      if (hit) {
        // Refresh LRU position.
        this.blocks.delete(b);
        this.blocks.set(b, hit);
        continue;
      }
      if (b * BLOCK_ROWS <= lastRow && (b + 1) * BLOCK_ROWS > firstRow) visible.push(b);
      else around.push(b);
    }
    this.wanted = [...visible, ...around];
    this.pump();
  }

  private pump() {
    while (!this.closed && this.inFlight.size < MAX_IN_FLIGHT && this.wanted.length) {
      const b = this.wanted.shift()!;
      if (this.blocks.has(b) || this.inFlight.has(b)) continue;
      this.fetch(b);
    }
  }

  private async fetch(b: number) {
    this.inFlight.add(b);
    try {
      const buf = await transport.data({ type: "rows", view: this.view, columns: null, offset: b * BLOCK_ROWS, limit: BLOCK_ROWS });
      if (this.closed) return;
      const table: ArrowTable = tableFromIPC(new Uint8Array(buf));
      const columns = new Map<string, Vector>();
      table.schema.fields.forEach((f, i) => columns.set(f.name, table.getChildAt(i)!));
      this.blocks.set(b, { rows: table.numRows, columns });
      while (this.blocks.size > MAX_BLOCKS) this.blocks.delete(this.blocks.keys().next().value!);
      this.onBlock(b * BLOCK_ROWS, b * BLOCK_ROWS + table.numRows - 1);
    } catch (e) {
      if (!this.closed) this.onError(typeof e === "object" && e && "message" in e ? String(e.message) : String(e));
    } finally {
      this.inFlight.delete(b);
      this.pump();
    }
  }
}

/** Fetches a range of rows for a few columns in one request (copying, exporting). */
export async function fetchRows(view: ViewSpec, columns: string[], offset: number, limit: number): Promise<ArrowTable> {
  const buf = await transport.data({ type: "rows", view, columns, offset, limit });
  return tableFromIPC(new Uint8Array(buf));
}
