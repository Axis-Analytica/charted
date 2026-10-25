import { Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useT } from "../i18n";
import { Dialog } from "../kit/Dialog";
import { Kbd } from "../kit/Kbd";
import { type Shortcut, useShortcuts } from "./shortcuts";

const GROUPS: Shortcut["group"][] = ["General", "File", "Edit", "View", "Navigate"];

export function ShortcutOverlay({ open, onClose }: { open: boolean; onClose(): void }) {
  const { t } = useT();
  const shortcuts = useShortcuts((s) => s.shortcuts);
  const groups = GROUPS.map((g) => [g, [...shortcuts.values()].filter((s) => s.group === g && s.keys && s.label)] as const).filter(([, items]) => items.length);
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title={t("Keyboard shortcuts")} width={640}>
      <div className="shortcut-grid">
        {groups.map(([group, items]) => (
          <section key={group}>
            <h3>{t(group)}</h3>
            <dl>
              {items.map((s) => (
                <div key={s.id} className="shortcut-row">
                  <dt>{t(s.label)}</dt>
                  <dd>
                    <Kbd keys={s.keys} />
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Dialog>
  );
}

export function CommandPalette({ open, onClose }: { open: boolean; onClose(): void }) {
  const { t } = useT();
  const shortcuts = useShortcuts((s) => s.shortcuts);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const items = useMemo(() => {
    const q = query.trim().toLowerCase();
    return [...shortcuts.values()]
      .filter((s) => s.id !== "palette" && s.label && (!s.enabled || s.enabled()))
      .filter((s) => !q || t(s.label).toLowerCase().includes(q) || s.label.toLowerCase().includes(q));
  }, [shortcuts, query, t]);
  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
    }
  }, [open]);
  const run = (s: Shortcut | undefined) => {
    if (!s) return;
    onClose();
    // Let the palette close before the command opens its own UI.
    setTimeout(() => s.run(), 0);
  };
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title={t("Commands")} width={560}>
      <div className="palette">
        <div className="palette-search">
          <Search size={15} />
          <input
            ref={input}
            autoFocus
            className="palette-input"
            placeholder={t("Type a command")}
            value={query}
            aria-label={t("Type a command")}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(items.length - 1, i + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(0, i - 1));
              } else if (e.key === "Enter") {
                e.preventDefault();
                run(items[index]);
              }
            }}
          />
        </div>
        <ul className="palette-list" role="listbox" aria-label={t("Commands")}>
          {items.map((s, i) => (
            <li key={s.id} role="option" aria-selected={i === index} className="palette-item" onMouseMove={() => setIndex(i)} onClick={() => run(s)}>
              <span>{t(s.label)}</span>
              {s.keys && <Kbd keys={s.keys} />}
            </li>
          ))}
          {items.length === 0 && <li className="palette-none">{t("No matching commands")}</li>}
        </ul>
      </div>
    </Dialog>
  );
}
