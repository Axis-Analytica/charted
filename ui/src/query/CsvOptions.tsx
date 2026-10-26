// A text file's settings: the file, its delimiter, encoding and decimal separator.
// "Automatic" lets charted work each out from the file (DuckDB's sniffer for the
// delimiter; the byte order mark, invalid UTF-8 and "1.234,5" numbers for the rest).
import { useState } from "react";
import { transport } from "../bridge";
import type { Query } from "../bridge/generated/Query";
import { useT } from "../i18n";
import { Button } from "../kit/Button";
import { Dialog } from "../kit/Dialog";
import { projectPath } from "./NewSource";

type Csv = Extract<Query["source"], { kind: "csv" }>;

const DELIMITERS = [
  { value: "", label: "Automatic" },
  { value: ",", label: "Comma" },
  { value: ";", label: "Semicolon" },
  { value: "\t", label: "Tab" },
  { value: "|", label: "Pipe" },
];
const ENCODINGS = [
  { value: "", label: "Automatic" },
  { value: "utf-8", label: "UTF-8" },
  { value: "utf-16", label: "UTF-16" },
  { value: "latin-1", label: "Western European (Windows)" },
];
const DECIMALS = [
  { value: "", label: "Automatic" },
  { value: ".", label: "Point (1,234.5)" },
  { value: ",", label: "Comma (1.234,5)" },
];

export function CsvOptions({ source, project, onDone }: { source: Csv; project: string; onDone(s: Csv | null): void }) {
  const { t } = useT();
  const [s, setS] = useState<Csv>(source);
  const choice = (label: string, value: string, options: { value: string; label: string }[], set: (v: string | null) => void) => (
    <label className="field">
      <span className="field-label">{label}</span>
      <select className="select" value={value} onChange={(e) => set(e.target.value || null)}>
        {options.map((o) => (
          <option key={o.label} value={o.value}>
            {t(o.label)}
          </option>
        ))}
      </select>
    </label>
  );
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onDone(null)}
      title={t("Text/CSV settings")}
      width={460}
      footer={
        <>
          <Button variant="ghost" onPress={() => onDone(null)}>
            {t("Cancel")}
          </Button>
          <Button variant="primary" onPress={() => onDone(s)}>
            {t("OK")}
          </Button>
        </>
      }
    >
      <div className="field">
        <span className="field-label">{t("File")}</span>
        <div className="csv-file">
          <span className="mono">{s.path}</span>
          <Button
            size="sm"
            variant="ghost"
            onPress={async () => {
              const path = await transport.pick({ title: t("Locate the source"), filters: [{ name: "CSV", extensions: ["csv", "tsv", "txt"] }] });
              if (path) setS((x) => ({ ...x, path: projectPath(project, path) }));
            }}
          >
            {t("Change…")}
          </Button>
        </div>
      </div>
      {choice(t("Delimiter"), s.delimiter ?? "", DELIMITERS, (v) => setS((x) => ({ ...x, delimiter: v })))}
      {choice(t("File encoding"), s.encoding ?? "", ENCODINGS, (v) => setS((x) => ({ ...x, encoding: v })))}
      {choice(t("Decimal separator"), s.decimal ?? "", DECIMALS, (v) => setS((x) => ({ ...x, decimal: v })))}
    </Dialog>
  );
}

