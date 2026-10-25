import type { ReactNode } from "react";
import { Button as AriaButton, Menu as AriaMenu, MenuItem as AriaMenuItem, MenuTrigger, Popover, Separator } from "react-aria-components";
import { Kbd } from "./Kbd";

export interface MenuEntry {
  id: string;
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  disabled?: boolean;
  run(): void;
}

/** A button that opens a list of commands. `null` entries draw separators. */
export function Menu({ label, trigger, items }: { label: string; trigger: ReactNode; items: (MenuEntry | null)[] }) {
  return (
    <MenuTrigger>
      <AriaButton className="btn btn-ghost btn-sm menu-trigger" aria-label={label}>
        {trigger}
      </AriaButton>
      <Popover className="menu-popover" placement="bottom start">
        <AriaMenu
          className="menu"
          disabledKeys={items.filter((i): i is MenuEntry => !!i && !!i.disabled).map((i) => i.id)}
          onAction={(key) => {
            const item = items.find((i) => i?.id === key);
            // Let the menu close before the command opens its own UI.
            if (item) setTimeout(() => item.run(), 0);
          }}
        >
          {items.map((i, n) =>
            i ? (
              <AriaMenuItem key={i.id} id={i.id} className="menu-item" textValue={i.label}>
                <span className="menu-icon">{i.icon}</span>
                <span className="menu-label">{i.label}</span>
                {i.shortcut && <Kbd keys={i.shortcut} />}
              </AriaMenuItem>
            ) : (
              <Separator key={`sep-${n}`} className="menu-sep" />
            ),
          )}
        </AriaMenu>
      </Popover>
    </MenuTrigger>
  );
}
