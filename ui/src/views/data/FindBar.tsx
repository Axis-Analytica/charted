import { ChevronDown, ChevronUp, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../../bridge";
import type { FindResult } from "../../bridge/generated/FindResult";
import type { ViewSpec } from "../../bridge/generated/ViewSpec";
import { formatNumber, useT } from "../../i18n";
import { IconButton } from "../../kit/Button";
import { Spinner } from "../../kit/misc";
import { dataJson } from "../../state/dataview";

/** Ctrl+F: finds text in any column of the view, in view order, wrapping at the ends. */
export function FindBar({ view, onFound, onClose }: { view: ViewSpec; onFound(r: FindResult): void; onClose(): void }) {
  const { t, tn } = useT();
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<FindResult | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);
  // A new view or text starts again from the top.
  useEffect(() => {
    setResult(null);
    setStatus(null);
  }, [view, text]);

  const find = async (backwards: boolean) => {
    if (!text || busy) return;
    setBusy(true);
    try {
      const ask = (after: number | null) => dataJson<FindResult | null>({ type: "find", view, text, columns: null, after, backwards });
      let r = await ask(result ? result.row : null);
      let wrapped = false;
      if (!r && result) {
        r = await ask(null);
        wrapped = true;
      }
      setResult(r);
      if (r) {
        onFound(r);
        setStatus(wrapped ? t("Wrapped around") : null);
      } else setStatus(t("No matches"));
    } catch (e) {
      setStatus(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="findbar" role="search">
      <Search size={14} className="findbar-icon" />
      <input
        ref={input}
        className="findbar-input"
        aria-label={t("Find in table")}
        placeholder={t("Find in table")}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") (e.preventDefault(), find(e.shiftKey));
          if (e.key === "Escape") onClose();
        }}
      />
      <span className="findbar-status" aria-live="polite">
        {busy ? <Spinner size={12} /> : status ?? (result ? tn("%n matching rows", result.matches, "%n matching row") : "")}
      </span>
      <IconButton label={t("Previous match")} shortcut="Shift+Enter" icon={<ChevronUp size={14} />} isDisabled={!text} onPress={() => find(true)} />
      <IconButton label={t("Next match")} shortcut="Enter" icon={<ChevronDown size={14} />} isDisabled={!text} onPress={() => find(false)} />
      <IconButton label={t("Close")} shortcut="Escape" icon={<X size={14} />} onPress={onClose} />
      <span className="visually-hidden">{result ? t("Row %1", formatNumber(result.row + 1)) : ""}</span>
    </div>
  );
}
