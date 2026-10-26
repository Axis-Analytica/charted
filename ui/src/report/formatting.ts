// Formatting several visuals at once: the format options their types share, their
// values (or "mixed"), and the format painter, which gives one visual another's look.
import type { Visual } from "../bridge/generated/Visual";
import { type ObjectDef, type PropDef, prop, visualDef } from "./registry";

/** A value that differs between the visuals being formatted. */
export const MIXED = Symbol("mixed");
export type Shown = unknown | typeof MIXED;

type Objects = Record<string, Record<string, unknown>>;

/** Props that hold a visual's content rather than its look: the format painter leaves them. */
const CONTENT: Record<string, string[]> = {
  title: ["text"],
  text: ["content"],
  image: ["src", "alt"],
  button: ["text"],
  action: ["type", "bookmark", "page", "tooltip"],
  syncGroup: ["groupName", "pages", "fieldChanges", "filterChanges"],
};
const isContent = (object: string, name: string) => CONTENT[object]?.includes(name) ?? false;

const sameProp = (a: PropDef, b: PropDef | undefined) => !!b && a.type === b.type;

/** The format sections every one of `types` has, with the props they all have. */
export function commonObjects(types: string[]): ObjectDef[] {
  const defs = [...new Set(types)].map(visualDef);
  if (!defs.length || defs.some((d) => !d)) return [];
  const [first, ...rest] = defs as NonNullable<(typeof defs)[number]>[];
  return first!.objects.flatMap((o) => {
    const others = rest.map((d) => d.objects.find((x) => x.name === o.name));
    if (others.some((x) => !x)) return [];
    const props = o.props.filter((p) => others.every((x) => sameProp(p, x!.props.find((q) => q.name === p.name))));
    if (!props.length) return [];
    const toggle = o.toggle && props.some((p) => p.name === o.toggle) ? o.toggle : undefined;
    return [{ ...o, toggle, props }];
  });
}

/** A prop's value across visuals: the shared value, or MIXED. */
export function shownValue(visuals: Visual[], object: string, name: string): Shown {
  const values = visuals.map((v) => prop<unknown>(v.objects, visualDef(v.visualType), object, name));
  const first = values[0];
  return values.every((x) => JSON.stringify(x) === JSON.stringify(first)) ? first : MIXED;
}

/** Sets one formatting prop on a visual (a slicer's style change clears its choice). */
export function withProp(v: Visual, object: string, name: string, value: unknown): Visual {
  const objects: Objects = { ...(v.objects ?? {}) };
  objects[object] = { ...(objects[object] ?? {}), [name]: value };
  const restyled = v.visualType === "slicer" && object === "slicer" && name === "style" && v.objects?.slicer?.style !== value;
  return { ...v, objects, selection: restyled ? null : v.selection };
}

/** `target` formatted like `source`: every format prop both types have takes the
 * source's value (left unset where it is the target's default). Content (titles' text,
 * a text box's words, an image) and data roles are kept. */
export function paintFormat(source: Pick<Visual, "visualType" | "objects">, target: Visual): Visual {
  const from = visualDef(source.visualType);
  const to = visualDef(target.visualType);
  if (!from || !to) return target;
  const objects: Objects = Object.fromEntries(Object.entries(target.objects ?? {}).map(([k, v]) => [k, { ...v }]));
  let restyled = false;
  for (const o of to.objects) {
    const src = from.objects.find((x) => x.name === o.name);
    if (!src) continue;
    for (const p of o.props) {
      if (isContent(o.name, p.name) || !sameProp(p, src.props.find((q) => q.name === p.name))) continue;
      const value = prop<unknown>(source.objects, from, o.name, p.name);
      if (o.name === "slicer" && p.name === "style" && value !== prop(target.objects, to, o.name, p.name)) restyled = true;
      const bag = (objects[o.name] ??= {});
      if (JSON.stringify(value) === JSON.stringify(p.default)) delete bag[p.name];
      else bag[p.name] = value;
      if (!Object.keys(bag).length) delete objects[o.name];
    }
  }
  return { ...target, objects, selection: restyled ? null : target.selection };
}
