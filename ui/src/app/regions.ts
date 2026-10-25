// F6 and Shift+F6 move the keyboard focus between the window's regions — the top
// bar, the view rail, the canvas or view, each pane, the status bar — as in Power BI
// and most desktop apps.
const REGIONS = "header.topbar, nav.rail, .report-center, main.main:not(:has(.report-center)), section.pane, aside.qe-settings, aside.qe-queries, footer.statusbar";
const FOCUSABLE = "button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex='-1'])";

export function cycleRegion(back: boolean) {
  const regions = [...document.querySelectorAll<HTMLElement>(REGIONS)].filter((r) => r.offsetParent !== null && !r.closest("[inert]"));
  if (!regions.length) return;
  const active = document.activeElement;
  const at = regions.findIndex((r) => active && r.contains(active));
  const next = regions[(at < 0 ? (back ? regions.length - 1 : 0) : at + (back ? -1 : 1) + regions.length) % regions.length]!;
  const target = [...next.querySelectorAll<HTMLElement>(FOCUSABLE)].find((e) => e.offsetParent !== null) ?? next;
  if (target === next && !next.hasAttribute("tabindex")) next.tabIndex = -1;
  target.focus();
}
