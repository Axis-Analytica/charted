// Visuals without data: a text box (markdown; double-click to edit), an image (stored
// in the report) and a shape.
import { ImagePlus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Page } from "../../bridge/generated/Page";
import type { Visual } from "../../bridge/generated/Visual";
import { errorMessage, transport } from "../../bridge";
import { useT } from "../../i18n";
import { Markdown } from "../../kit/Markdown";
import { useApp } from "../../state/app";
import { toast } from "../../state/toasts";
import { editVisual } from "../../state/report";
import { prop, visualDef } from "../registry";

const setObject = (page: Page, visual: Visual, object: string, values: Record<string, unknown>, label: string) =>
  useApp.getState().edit(
    label,
    editVisual(page.id, visual.id, (v) => ({ ...v, objects: { ...v.objects, [object]: { ...(v.objects?.[object] ?? {}), ...values } } })),
  );

export function TextBox({ visual, page }: { visual: Visual; page: Page }) {
  const { t } = useT();
  const def = visualDef("textbox");
  const content = prop<string>(visual.objects, def, "text", "content");
  const [editing, setEditing] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (editing) ref.current?.focus();
  }, [editing]);
  const style = {
    fontSize: prop<number>(visual.objects, def, "text", "fontSize"),
    fontWeight: prop<boolean>(visual.objects, def, "text", "bold") ? 700 : 400,
    textAlign: prop<"left" | "center" | "right">(visual.objects, def, "text", "align"),
    color: prop<string>(visual.objects, def, "text", "color") || undefined,
  };
  if (editing)
    return (
      <textarea
        ref={ref}
        className="textbox-edit"
        defaultValue={content}
        aria-label={t("Text")}
        style={style}
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Escape") setEditing(false);
        }}
        onBlur={(e) => {
          setEditing(false);
          if (e.target.value !== content) setObject(page, visual, "text", { content: e.target.value }, "Edit text");
        }}
      />
    );
  return (
    <div className={`textbox ${content ? "" : "is-empty"}`} style={style} onDoubleClick={() => setEditing(true)}>
      {content ? <Markdown text={content} /> : t("Double-click to type")}
    </div>
  );
}

const MIME: Record<string, string> = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", svg: "image/svg+xml" };

/** Asks for an image file and returns it as a data URL. */
async function chooseImage(title: string): Promise<string | null> {
  const path = await transport.pick({ title: title, filters: [{ name: "Images", extensions: Object.keys(MIME) }] });
  if (!path) return null;
  const bytes = await transport.data({ type: "readImage", path });
  const type = MIME[path.split(".").pop()?.toLowerCase() ?? ""] ?? "application/octet-stream";
  return await new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsDataURL(new Blob([bytes], { type }));
  });
}

export async function placeImage(page: Page, visual: Visual, title: string) {
  try {
    const src = await chooseImage(title);
    if (src) await setObject(page, visual, "image", { src }, "Set image");
  } catch (e) {
    toast({ kind: "error", title: errorMessage(e) });
  }
}

export function ImageVisual({ visual, page }: { visual: Visual; page: Page }) {
  const { t } = useT();
  const def = visualDef("image");
  const src = prop<string>(visual.objects, def, "image", "src");
  const fit = prop<string>(visual.objects, def, "image", "fit");
  if (!src)
    return (
      <button type="button" className="image-empty" onPointerDown={(e) => e.stopPropagation()} onClick={() => placeImage(page, visual, t("Choose an image"))}>
        <ImagePlus size={28} strokeWidth={1.25} />
        <span>{t("Choose an image")}</span>
      </button>
    );
  return <img className="image-visual" src={src} alt={prop<string>(visual.objects, def, "image", "alt")} style={{ objectFit: fit as "contain" | "cover" | "fill" }} draggable={false} />;
}

export function Shape({ visual }: { visual: Visual }) {
  const def = visualDef("shape");
  const kind = prop<string>(visual.objects, def, "shape", "kind");
  const fill = prop<string>(visual.objects, def, "shape", "fill") || "var(--accent-soft)";
  const stroke = prop<string>(visual.objects, def, "shape", "outline") || "var(--accent)";
  const width = prop<number>(visual.objects, def, "shape", "weight");
  const { width: w, height: h } = visual.position;
  const s = width / 2;
  return (
    <svg className="shape-visual" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" aria-hidden>
      {kind === "ellipse" && <ellipse cx={w / 2} cy={h / 2} rx={Math.max(0, w / 2 - s)} ry={Math.max(0, h / 2 - s)} fill={fill} stroke={stroke} strokeWidth={width} />}
      {(kind === "rectangle" || kind === "roundedRectangle") && (
        <rect x={s} y={s} width={Math.max(0, w - width)} height={Math.max(0, h - width)} rx={kind === "roundedRectangle" ? Math.min(w, h) * 0.15 : 0} fill={fill} stroke={stroke} strokeWidth={width} />
      )}
      {kind === "line" && <line x1={0} y1={h / 2} x2={w} y2={h / 2} stroke={stroke} strokeWidth={width} />}
      {kind === "arrow" && <polygon points={`0,${h * 0.3} ${w * 0.65},${h * 0.3} ${w * 0.65},0 ${w},${h / 2} ${w * 0.65},${h} ${w * 0.65},${h * 0.7} 0,${h * 0.7}`} fill={fill} stroke={stroke} strokeWidth={width} strokeLinejoin="round" />}
    </svg>
  );
}
