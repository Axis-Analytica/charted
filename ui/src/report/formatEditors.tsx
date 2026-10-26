// Format pane sections with editors of their own (registry objects with `editor`):
// conditional formatting for tables and matrices, and a chart's tooltip page.
import type { Page } from "../bridge/generated/Page";
import type { Visual } from "../bridge/generated/Visual";
import { useApp } from "../state/app";
import { editVisual } from "../state/report";
import { ConditionalFormattingSection } from "./ConditionalFormatting";
import { TooltipPagePicker } from "./tooltipPages";

export function FormatEditor({ editor, page, visual }: { editor: string; page: Page; visual: Visual }) {
  const model = useApp((s) => s.state?.project?.model);
  const edit = useApp((s) => s.edit);
  if (!model) return null;
  switch (editor) {
    case "conditionalFormatting":
      return <ConditionalFormattingSection model={model} page={page} visual={visual} />;
    case "tooltipPage":
      return (
        <TooltipPagePicker
          model={model}
          page={page}
          visual={visual}
          onChange={(value) => edit("Format visual", editVisual(page.id, visual.id, (v) => ({ ...v, objects: { ...(v.objects ?? {}), tooltip: { ...(v.objects?.tooltip ?? {}), page: value } } })))}
        />
      );
    default:
      return null;
  }
}
