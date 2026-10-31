// A toolbar button people can hide: right-click offers "Hide … from the toolbar", and a
// hidden one isn't drawn (it comes back from the Layout menu, and stays in the palette).
import { type ReactNode, useState } from "react";
import { useT } from "../i18n";
import { ContextMenu, type ContextMenuState } from "../kit/ContextMenu";
import { hideable } from "./commands";
import { setLayout, useLayout, withHidden } from "./layout";

export function Hideable({ id, children }: { id: string; children: ReactNode }) {
  const { t } = useT();
  const hidden = useLayout().hidden.includes(id);
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  if (hidden) return null;
  return (
    <span
      className="toolbar-command"
      data-command={id}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({
          x: e.clientX,
          y: e.clientY,
          label: t("Toolbar"),
          items: [{ id: "hide", label: t("Hide %1 from the toolbar", t(hideable(id)?.label ?? id)), run: () => void setLayout((l) => withHidden(l, [id], true)) }],
        });
      }}
    >
      {children}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </span>
  );
}
