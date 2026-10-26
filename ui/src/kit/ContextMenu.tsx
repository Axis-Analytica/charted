// A menu opened at the pointer (right-click), or under the focused item (Shift+F10 or
// the Menu key). Arrow keys move, Enter runs, Escape or a click elsewhere closes.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { MenuEntry } from "./Menu";

export interface ContextMenuState {
  x: number;
  y: number;
  label: string;
  /** `null` entries draw separators. */
  items: (MenuEntry | null)[];
}

export function ContextMenu({ menu, onClose }: { menu: ContextMenuState; onClose(): void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: menu.x, top: menu.y });
  // Kept on screen.
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ left: Math.max(4, Math.min(menu.x, window.innerWidth - r.width - 4)), top: Math.max(4, Math.min(menu.y, window.innerHeight - r.height - 4)) });
  }, [menu.x, menu.y]);
  useEffect(() => {
    const back = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("[role=menuitem]:not(:disabled)")?.focus();
    const close = (e: Event) => !ref.current?.contains(e.target as Node) && onClose();
    window.addEventListener("mousedown", close, true);
    return () => {
      window.removeEventListener("mousedown", close, true);
      if (back?.isConnected) back.focus();
    };
  }, [onClose]);
  const move = (by: number | "first" | "last") => {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>("[role=menuitem]:not(:disabled)") ?? [])];
    if (!items.length) return;
    const i = items.indexOf(document.activeElement as HTMLElement);
    items[by === "first" ? 0 : by === "last" ? items.length - 1 : (i + by + items.length) % items.length]?.focus();
  };
  return (
    <div
      ref={ref}
      className="visual-menu point-menu context-menu"
      role="menu"
      aria-label={menu.label}
      style={{ left: pos.left, top: pos.top }}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") move(1);
        else if (e.key === "ArrowUp") move(-1);
        else if (e.key === "Home") move("first");
        else if (e.key === "End") move("last");
        else if (e.key === "Escape" || e.key === "Tab") onClose();
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {menu.items.map((i, n) =>
        i ? (
          <button
            key={i.id}
            type="button"
            role="menuitem"
            disabled={i.disabled}
            onClick={() => {
              onClose();
              // Let the menu close before the command opens its own UI.
              setTimeout(() => i.run(), 0);
            }}
          >
            {i.icon}
            <span className="menu-label">{i.label}</span>
            {i.shortcut && <kbd className="context-menu-key">{i.shortcut}</kbd>}
          </button>
        ) : (
          <div key={`sep-${n}`} className="menu-sep" role="separator" />
        ),
      )}
    </div>
  );
}
