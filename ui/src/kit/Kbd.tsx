const MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform || navigator.userAgent);
const MAC_KEYS: Record<string, string> = { Ctrl: "⌘", Alt: "⌥", Shift: "⇧" };

/** A shortcut as the keyboard shows it: on macOS "Ctrl+Shift+Z" is "⌘⇧Z". */
export function keyLabel(keys: string): string {
  if (!MAC || keys === "+") return keys;
  return keys
    .split("+")
    .map((k) => MAC_KEYS[k] ?? k)
    .join("");
}

/** Renders a shortcut such as "Ctrl+Shift+Z" as key caps (⌘ ⇧ Z on macOS). */
export function Kbd({ keys }: { keys: string }) {
  const parts = keys === "+" ? ["+"] : keys.split("+").map((k) => (MAC ? (MAC_KEYS[k] ?? k) : k));
  return (
    <span className="kbd" aria-label={keyLabel(keys)}>
      {parts.map((k, i) => (
        <kbd key={`${k}${i}`}>{k}</kbd>
      ))}
    </span>
  );
}
