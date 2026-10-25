import { FolderOpen, Save, Sparkles, Trash2 } from "lucide-react";
import { useState } from "react";
import { useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { ConfirmDialog } from "../kit/Dialog";
import { Kbd } from "../kit/Kbd";
import { Badge, Banner, EmptyState, Spinner } from "../kit/misc";
import { toast } from "../state/toasts";

const TOKENS = ["--bg", "--bg-sunken", "--bg-raised", "--bg-panel", "--bg-hover", "--bg-active", "--fg", "--fg-strong", "--fg-muted", "--fg-faint", "--border", "--border-strong", "--selection", "--accent", "--accent-soft", "--accent-text", "--danger", "--warning", "--success", "--info"];

/** Every kit component in its states, rendered with the live theme (development aid, Ctrl+Alt+K). */
export function KitGallery({ onClose }: { onClose(): void }) {
  const { t } = useT();
  const [confirm, setConfirm] = useState(false);
  const chart = Array.from({ length: 10 }, (_, i) => `--chart-${i + 1}`).filter((v) => getComputedStyle(document.documentElement).getPropertyValue(v).trim());
  return (
    <div className="gallery" role="dialog" aria-label={t("Component kit")}>
      <header className="gallery-head">
        <strong>{t("Component kit")}</strong>
        <Button variant="ghost" onPress={onClose}>
          {t("Close")} <Kbd keys="Esc" />
        </Button>
      </header>
      <div className="gallery-body">
        <section>
          <h3>Buttons</h3>
          <div className="gallery-row">
            <Button variant="primary" icon={<FolderOpen size={16} />}>Primary</Button>
            <Button icon={<Save size={16} />}>Secondary</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="danger" icon={<Trash2 size={16} />} onPress={() => setConfirm(true)}>Danger</Button>
            <Button isDisabled>Disabled</Button>
            <Button size="sm">Small</Button>
            <Button size="lg" icon={<Sparkles size={17} />}>Large</Button>
          </div>
          <div className="gallery-row">
            <IconButton label="Save" shortcut="Ctrl+S" icon={<Save size={16} />} />
            <IconButton label="Pressed" icon={<Sparkles size={16} />} pressed />
            <IconButton label="Disabled" icon={<Trash2 size={16} />} isDisabled />
            <Kbd keys="Ctrl+Shift+Z" />
            <Spinner size={16} />
          </div>
        </section>
        <section>
          <h3>Badges and banners</h3>
          <div className="gallery-row">
            <Badge>Neutral</Badge>
            <Badge tone="accent">Accent</Badge>
            <Badge tone="warning">Warning</Badge>
            <Badge tone="danger">Danger</Badge>
          </div>
          <div className="gallery-stack">
            <Banner tone="info">An informational banner.</Banner>
            <Banner tone="warning" actions={<Button size="sm">Act</Button>}>A warning with an action.</Banner>
            <Banner tone="danger">Something went wrong and needs attention.</Banner>
          </div>
        </section>
        <section>
          <h3>Fields and toasts</h3>
          <div className="gallery-row">
            <label className="field">
              <span className="field-label">Name</span>
              <input className="input" defaultValue="Total Sales" />
            </label>
            <Button onPress={() => toast({ kind: "success", title: "Saved Harbor & Pine" })}>Success toast</Button>
            <Button onPress={() => toast({ kind: "error", title: "Could not read data/sales.parquet", detail: "The file was moved or deleted." })}>Error toast</Button>
          </div>
        </section>
        <section>
          <h3>Empty state</h3>
          <EmptyState icon={<Sparkles size={28} />} title="No visuals yet" actions={<Button variant="primary">Add a visual</Button>}>
            Drag a field onto the canvas, or press C for a column chart.
          </EmptyState>
        </section>
        <section>
          <h3>Tokens</h3>
          <div className="swatches">
            {TOKENS.map((v) => (
              <div key={v} className="swatch">
                <span className="swatch-chip" style={{ background: `var(${v})` }} />
                <code>{v}</code>
              </div>
            ))}
          </div>
          <h3>Chart palette</h3>
          <div className="swatches">
            {chart.map((v) => (
              <div key={v} className="swatch">
                <span className="swatch-chip" style={{ background: `var(${v})` }} />
                <code>{v}</code>
              </div>
            ))}
          </div>
        </section>
      </div>
      <ConfirmDialog
        open={confirm}
        title="Delete measure?"
        message="Total Sales is used by 3 visuals. They will show an error until you choose another field."
        confirm="Delete"
        cancel="Cancel"
        danger
        onConfirm={() => setConfirm(false)}
        onCancel={() => setConfirm(false)}
      />
    </div>
  );
}
