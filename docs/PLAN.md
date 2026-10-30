# Plan after v1.5.0: launch, M8 (reshape the tool), and beyond

Written 2026-10-31, after v1.5.0 shipped the in-app agent (M7), alongside the launch post
"The Sincerest Form of Flattery". Its aim: make the post's promises true, then build
the next one it makes, a BI tool you can reshape around yourself. Read docs/HANDOFF.md
for the current state and docs/ROADMAP.md for the long-range scope.

Marked **(go-ahead)**: needs the user's explicit OK before it happens. Each stage ends
green on `bin/test` and is committed. Every user-visible stage gets a demo note in
`docs/demos/` and a checkpoint.

## Stage L: launch readiness (now, before the post goes out)

The post promises that you can download it today, that it works with your AI, and that
people can tell us what they love and hate. These make that true.

1. **v1.5.0 released.** Tag, CI tarballs, published release, install line in the README.
2. **Install in one command.**
   - Publish `charted-bin` (and `charted`) to the AUR. This needs the user's AUR SSH key
     on this machine. **(go-ahead)**
   - Open the omarchy-pkgs PR. **(go-ahead)**
   - Until then, the README route stays `makepkg -si`.
3. **Feedback intake.**
   - `.github/ISSUE_TEMPLATE/`: bug (version, Omarchy version, steps, project or PBIP if
     shareable), "doesn't import" (PBIP feature, import report), and idea.
   - Turn on GitHub Discussions. **(go-ahead)**
   - Link both from the app: Help menu → *Report a problem* (the crash flow already
     opens a prefilled issue).
4. **"What comes across" page** (`docs/powerbi.md`). What imports and exports, what
   doesn't yet, and the DAX support list (`docs/dax-support.md`). Early users will bring
   their own PBIP files; tell them before they find out.
5. **Contributor files.**
   - `CONTRIBUTING.md`: setup, `bin/heavy`, `bin/test`, commit style, the e2e
     shared-sample rule.
   - `AGENTS.md` (and a `CLAUDE.md` pointer): the same rules in agent form (never run
     prettier on `ui/`, regenerate bindings and catalog, i18n catalogs append-only).
   - The post invites agent-assisted contributors, so this matters.
6. **Launch media.** A short recording of the Agent pane building the roadmap page with
   real Claude Code. Use `CHARTED_SHOTS` on `e2e/m7agent.spec.ts` for stills, and a
   screen recording of the app for the GIF.
7. **CI flakes.** `v11model` (hierarchy drill) and `v12` (Top N) time out on GitHub's
   runners. Make them wait for data the same way the v13 and m6 fixes did.
8. **Real-world checks the post makes urgent.**
   - The Power BI Desktop round trip in the Windows VM
     (`~/Work/charted-checkpoints/pbi-roundtrip/`). **(the user runs it)**
   - The Codex checkpoint, once Codex is signed in.
   - OpenCode in terminal mode.

## M8: reshape the tool (the post's "Where it's headed")

**Goal.** The user, or their agent, can change how charted itself is laid out:
- floating or docked panes;
- buttons and commands hidden;
- model and report side by side.

Changes persist, can be undone, and can be shared as a profile.

**Why it's a milestone, not a setting.** There is no UI-preferences store today. Pane
state is in memory (`useReport.panes`), views are exclusive (`useReport.view`), and the
toolbar is hard-coded. M8 builds a layout layer first, then gives it to the agent.

### M8.1 A workspace layout model (core)

- **`Workspace` type** (ts-rs), kept per user in `<state dir>/workspace.json` and
  optionally per project (`.charted` manifest `workspace` override):
  - `panes`: id → `{ placement: dock-left | dock-right | float | hidden, width,
    float: {x, y, w, h}, order }`;
  - `views`: the layout of the main area: a single view, or a split
    (`{ split: "h" | "v", ratio, a: view, b: view }`), where a view is report, data,
    model or query;
  - `hidden`: command ids (toolbar buttons and menu items) to hide;
  - `toolbar`: order overrides;
  - `density` and font size overrides.
- **Requests.** `Request::SetWorkspace { patch }` and `ResetWorkspace`.
- **Undo.** Its own small undo history, apart from the project's: layout isn't the
  model. *Undo layout* appears in the View menu and in the agent's steps.
- **Profiles.** Save, load, export and import `.charted-workspace` files.

### M8.2 The UI reads the layout

- **Panes.** Every pane (Fields, Visualizations, Filters, Bookmarks, Selection, Sync,
  Agent; plus the Data view's Stats drawer and the Model view's properties) renders
  through one `PaneHost`. It docks left or right with a resize handle, or floats in a
  draggable, resizable overlay inside the window (no separate OS windows; that keeps
  Hyprland and Tauri simple).
- **Split views.** The main area renders the `views` tree. Report beside model is the
  headline. Two views share the selection: picking a table in the model highlights its
  fields in the report.
- **Hiding commands.** One command registry (extend `app/shortcuts.ts`'s registry into
  a command list with ids, labels and the toolbar or menu they appear in). Toolbars and
  menus render from it and skip hidden ids. Everything stays reachable from the command
  palette (Ctrl+K), so nothing is lost for good.
- **Hand editing.**
  - Drag pane headers to dock or float them.
  - Right-click a toolbar button → *Hide*.
  - View → *Split right with…*.
  - View → *Customize…* lists hidden commands to bring back.
- **Compact mode** (the existing tabbed pane stack under 1240 px) becomes one layout
  the user can also choose.

### M8.3 The agent reshapes the tool

- **New tools** on the same socket:
  - `describe_workspace`;
  - `set_pane { pane, placement, width? }`;
  - `split_view { a, b, direction, ratio }`;
  - `hide_commands { ids }` / `show_commands`;
  - `list_commands` (ids, labels, where they appear, and how often the user used each:
    a local usage count kept in the state dir, never sent anywhere unless the agent
    asks).
- **Workflow.** "Hide the buttons I never use" makes the agent call `list_commands` and
  propose. That's a confirm card, like deletes, because hiding things is surprising.
  Each request is one layout-undo step.
- **Prompt and tests.** Update the prompt, `docs/demos/m8.md`, and e2e
  `m8layout.spec.ts` with the stand-in agent (float a pane, split report and model,
  hide and restore a button, undo).

### M8.4 Polish

- Layout survives restarts and project switches.
- Keyboard: focus moves between split views with F6.
- Right-to-left mirrors docks.
- Reduced motion is respected when panes float or snap.

**Acceptance (checkpoint).** Ask the agent: "Float the fields pane, put the model view
beside the report, and hide the buttons I've never pressed." Watch it happen live, undo
it in one step, then save it as a profile and load it again.

## M9: a deeper agent

- **Seeing the page.** A `screenshot_page` tool renders a page to PNG (the PDF export
  path already renders pages), so the agent can check layout and legibility. Privacy:
  a screenshot shows data, so it is gated by "Let the agent see data".
- **Project instructions.** `.charted/agent.md` per project: naming rules, colours,
  "always use the Date table". The agent reads it every request, and the pane edits it.
- **Agent-written formatting standards.** For example: "make all titles sentence case
  and 14 px across the report".
- **`charted mcp --project DIR`.** The tools on a project without the window open, for
  agents run elsewhere (CI, scripts). It holds the project lock, so it refuses while the
  app has the project open.
- **Evaluations.** A small suite of real-agent prompts on the sample (build a page, a
  YoY measure, tidy a page, a calculation group). They are scored by checks on the
  resulting model and run on demand, like the corpus run. They catch prompt or tool
  regressions and compare models.
- **More agents in the pane.** OpenCode (`opencode run --format json`) and others whose
  headless output can be parsed, each with its own built-in-tool lockdown, verified
  before it's offered.

## M10: Mac and Windows (the post's wishlist)

- **Audit what's Omarchy-specific.**
  - Theme following: Omarchy's theme files, with a fallback to the OS's light or dark.
  - `omarchy-default-agent` and the account homes: fall back to whichever agents are
    on PATH.
  - The Hyprland IPC for animations.
  - Paths: XDG, with Tauri path APIs on other platforms.
  - The control socket: a Unix socket on Mac; a named pipe on Windows.
  - Process groups for Stop: a job object on Windows.
- **Build and test.** CI builds for macOS (universal) and Windows (MSI). The DuckDB
  prebuilt library exists for both. Run e2e on each.
- **Distribution.** Signing and notarisation need the user's Apple developer and Windows
  code-signing decisions. **(go-ahead)**

## Alongside: the v1.x quality track

- DAX coverage from what users report, with golden cases and a regenerated
  `docs/dax-support.md`.
- Import robustness from user PBIP files, via the public corpus and new failures.
- Performance: window-function rewrites for EARLIER ranks on 100k+ rows if users hit
  them (docs/perf.md).

## Order and rough size

| Stage | Depends on | Size |
|---|---|---|
| L (launch readiness) | v1.5.0 | about 1 session, plus the user's go-aheads |
| M8.1–M8.2 (layout model and UI) | – | about 2 sessions |
| M8.3–M8.4 (agent tools and polish) | M8.2 | about 1 session; release as v1.6.0 |
| M9 (deeper agent) | M7 | about 2 sessions; v1.7.0 |
| M10 (Mac and Windows) | – | about 2 sessions, plus signing decisions |

## Decisions for the user

1. AUR and omarchy-pkgs: publish now? (needs the AUR key)
2. GitHub Discussions on?
3. M8 floating panes: floating inside the window (recommended), or real OS windows?
4. A per-project layout override, or per-user only?
5. Start M10 before or after M9?
