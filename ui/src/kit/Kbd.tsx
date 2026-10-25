/** Renders a shortcut such as "Ctrl+Shift+Z" as key caps. */
export function Kbd({ keys }: { keys: string }) {
  const parts = keys === "+" ? ["+"] : keys.split("+");
  return (
    <span className="kbd" aria-label={keys}>
      {parts.map((k, i) => (
        <kbd key={`${k}${i}`}>{k}</kbd>
      ))}
    </span>
  );
}
