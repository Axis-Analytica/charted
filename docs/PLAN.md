# Plan after v1.5.0: launch, M8 (reshape the tool), and beyond

Written 2026-10-31, after v1.5.0 shipped the in-app agent (M7), alongside the launch post
"The Sincerest Form of Flattery". Its aim: make the post's promises true, then build
the next one it makes, a BI tool you can reshape around yourself. Read docs/HANDOFF.md
for the current state and docs/ROADMAP.md for the long-range scope.

Marked **(go-ahead)**: needs the user's explicit OK before it happens. Each stage ends
green on `bin/test` and is committed. Every user-visible stage gets a demo note in
`docs/demos/` and a checkpoint.

## Stage L: launch readiness (now, before the post goes out)

**Status 2026-10-31:**
- **Done:** items 1, 3 to 5 and 7.
  - v1.5.0 released.
  - Issue templates (bug, Power BI import/export), Discussions on, and *Report a problem…*
    and *Ideas and questions…* in the File menu and Ctrl+K.
  - docs/powerbi.md.
  - CONTRIBUTING.md and AGENTS.md (CLAUDE.md points to it).
  - The v11model and v12 CI flakes fixed.
- **Item 2:** omarchy-pkgs#1879 is open; the AUR waits for the user's key.
- **Item 6** (media) and **item 8** (VM, Codex, OpenCode checks) are the user's.

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

**Done (2026-10-31):** M8.1 to M8.3; the demo is in docs/demos/m8.md.
- `crates/core/src/workspace.rs`: profiles, layout, layout undo.
- `ui/src/app/layout.ts` and `layoutModel.ts`; `report/PaneHost.tsx`;
  `app/LayoutMenu.tsx`.
- The split view in `views/ProjectView.tsx`.
- `crates/agent/src/tools/workspace.rs`.

Simplified from the plan below:
- Floating panes stay inside the report area.
- The split is one second view (the left side is the rail's view).
- Hideable commands are the report toolbar's for now. File menu items and other views'
  toolbars are next.
- Export and import of profiles as files, and keyboard focus between split views, are
  still open (M8.4).

**Goal.** The user, or their agent, can change how charted itself is laid out:
- floating or docked panes;
- buttons and commands hidden;
- model and report side by side.

Changes persist, can be undone, and can be shared as a profile.

**Why it's a milestone, not a setting.** There is no UI-preferences store today. Pane
state is in memory (`useReport.panes`), views are exclusive (`useReport.view`), and the
toolbar is hard-coded. M8 builds a layout layer first, then gives it to the agent.

### M8.1 A workspace layout model (core)

- **`Workspace` type** (ts-rs), kept per user in `<state dir>/workspaces/`. There is no
  per-project layout. A user has several named **profiles** (for example *Standard*,
  *Report + model*, *Two report pages*), one active at a time, switched from the View
  menu, the command palette and the agent. A profile holds:
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
- **Profiles.** Create, duplicate, rename, delete and switch profiles; export and import
  them as `.charted-workspace` files. Built-in starting points:
  - **Standard:** today's layout.
  - **Report + model:** a split view.
  - **Two report pages:** two report views side by side, each with its own page.

  These are copied on first edit, so the built-ins stay as they ship.

### M8.2 The UI reads the layout

- **Panes.** Every pane (Fields, Visualizations, Filters, Bookmarks, Selection, Sync,
  Agent; plus the Data view's Stats drawer and the Model view's properties) renders
  through one `PaneHost`. It docks left or right with a resize handle, or floats in a
  draggable, resizable overlay inside the window (no separate OS windows; that keeps
  Hyprland and Tauri simple).
- **Split views.** The main area renders the `views` tree. Report beside model is the
  headline; two report views each keep their own current page (the page selection moves
  from `useReport` into each view's own state). Two views share the selection: picking a table in the model highlights its
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
  - `profile { action: create | switch | rename | delete, name }`;
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

## Clean-up before M9 (done)

The loose ends from M7, M8 and launch, in a sensible order.

**Waiting on the user**
1. **AUR.** Add `~/.ssh/aur.pub` to the AUR account, then run `bin/aur-publish` (it does
   `charted` and `charted-bin`).
2. **v1.6.0 is released** (2026-10-31). The PKGBUILDs carry its checksums, so
   `bin/aur-publish` publishes 1.6.0.
3. **omacom/omarchy-pkgs#1879** (`charted-bin`, updated to 1.6.0) waits for a
   maintainer's build approval.
   Answer review comments if any.
4. **Checks only the user can run:**
   - the Power BI Desktop round trip in the VM;
   - signing in to Codex (then run the Codex checkpoint);
   - launch media (a GIF of the Agent pane).

**Done (2026-10-31, after v1.6.0)**
5. Second report view: drill through, bookmarks, page buttons, Back, the agent's follow
   and shortcuts act on the report view last used; panes show in the second view when
   the first isn't a report; stacked panes keep their header actions (Add bookmark).
6. M8 leftovers: File-menu items and the Data, Model and Transform data buttons hide
   (Layout > Buttons and menus); profiles export and import as `.charted-workspace`;
   F6 reaches the second view; right-to-left dock labels and the float corner grip.
7. Agent pane: recovered failures show as retried; replies are asked to stay short and
   not narrate; `query_visual` says how rows are sorted (and flags a text axis with no
   sort-by column).
8. Terminal mode starts the picked agent (not Omarchy's default) with its MCP set-up;
   only OpenCode, Cursor CLI, Copilot and Gemini CLI are offered. **Still to do:** run
   one end to end (none is signed in here; Copilot is installed).
9. `bin/clean` frees build caches.
10. CI flakes: the pattern is to wait for the data, never to retry. Watch for new ones.

## M9: a deeper agent

**Done (2026-10-31), not released.** All below except more agents in the pane. See
docs/HANDOFF.md (M9) and docs/demos/m9.md.

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

## v1.7.0: feedback batch (the next session starts here)

Planned 2026-10-31 with the user. Progress is marked in each stage's heading.

The user installed v1.6.0 and tried the M9 agent: "able to whip up some excellent reports
from a single prompt". Their feedback, with the open project (Harbor & Pine checkpoint,
agent chat in `.cache/agent/chat.json`) as evidence:

1. Markdown (bold, italics, …) in the agent chat and in the text objects it builds.
2. Fonts: 10–15 common fonts, plus installing your own. The agent itself told the user
   "I can't change the font to Helvetica… tools set colours but not font".
3. Slicers repeat their name ("Store Store").
4. Also from that chat: "There's no layer-order control, so I'll … re-create the slicers
   last so they sit on top". Add z-order.
5. A GIF of the agent building a dashboard, for the README and a LinkedIn post.
6. Then release v1.7.0 (M9 + this batch), then M10 (Mac and Windows).

User's answers: fixes go into 1.7.0; update omarchy-pkgs #1879 to 1.7.0 too (AUR only if
the key is registered); user fonts live in their charted font library *and* get copied
into the project when used; GIF = the agent building a dashboard (GIF + MP4).

Each stage ends green on `bin/heavy bin/test` and is committed.

### Stage 1: slicer name shown once (and layer order for the agent) — done

Cause: `set_title` (`crates/agent/src/tools/report.rs:467`) always writes
`title={show:true,text}`; the slicer also draws its own header with the field name
(`ui/src/report/visuals/Slicer.tsx:31-46`, header on by default in `registry.ts:389`).
So "Store" appears twice.

- Registry: slicer `header` gains `text` and `fontSize` (Power BI's slicer header has
  them). `Slicer.tsx` shows `header.text ?? field name`.
- Agent: for slicers, `set_title` writes `header.text` and leaves the container title
  off (`add_visual`, `update_visual`, `format_visuals` all go through it). Regenerate
  `catalog.json` (parity test).
- Existing reports (like the user's): when a slicer's container title is shown and its
  header would show the same text, draw the header without the text (keep the clear
  button). No migration needed.
- PBIR (`crates/tmdl/src/pbir.rs` ~485 / ~920): export and import slicer
  `objects.header.show/text`, so Power BI files keep it.
- Layer order: `update_visual` takes `layer: "front"|"back"|"forward"|"backward"`;
  `describe_report` prints z (`tools/read.rs:325`); `arrange_page` keeps the existing
  relative z instead of renumbering by band (`layout.rs:125`). Prompt line.
- Tests: Rust test that an agent-made slicer has no container title and header text;
  e2e: a slicer with title+header shows the name once; Vitest for the dedupe.

### Stage 2: markdown — done (links show, but don't open)

No markdown library exists; write a small safe renderer, `ui/src/kit/Markdown.tsx`,
that parses a subset into React nodes (never `innerHTML`): paragraphs, line breaks,
`**bold**`, `*italic*`/`_italic_`, `~~strike~~`, `` `code` ``, `[link](https://…)`
(opens externally via the existing opener path), `- `/`1. ` lists, `#`–`###` headings.
Vitest for the parser (including escapes and unclosed markers).

- Agent pane: `AgentPane.tsx:253` renders text items with it (`agent.css:20`). Relax
  `prompt.md:42-45`: bold for names and short lists are fine; still no headings or
  tables in chat.
- Text box: `objects.text.content` is markdown (plain text stays valid). `Decor.tsx`
  renders through `Markdown`, the textarea editing stays; the format pane's text field
  becomes a multi-line textarea (`VisualizationsPane.tsx:273`). Headings take sizes
  relative to `text.fontSize`.
- PBIR round trip: export splits markdown into paragraphs and `textRuns` with
  `textStyle` (`fontWeight: bold`, `fontStyle: italic`, `textDecoration`, `url`);
  import turns styled runs back into markdown (`pbir.rs:507`, `:938`). Rust tests.
- Agent: prompt says text boxes take markdown; `add_visual type:"textbox"` documented.
- Titles stay plain text (Power BI titles are plain).

### Stage 3: fonts — done

- **Bundled (15, all OFL, from `@fontsource`/`@fontsource-variable`, latin subset,
  400/700 or variable):** Inter (already), Roboto, Open Sans, Lato, Montserrat,
  Poppins, Source Sans 3, Noto Sans, Nunito, IBM Plex Sans, Oswald, Merriweather, Lora,
  Playfair Display, Roboto Mono. Imported in a new `ui/src/design/fonts.ts`, offline.
  Power BI names (Segoe UI, Arial, Helvetica, DIN, Georgia, …) stay selectable and get
  sensible fallbacks to a bundled font in a `FONT_FALLBACKS` map.
- **Your fonts:** "Add a font…" (theme editor and font pickers) picks .ttf/.otf/.woff2.
  Core (`crates/core/src/fonts.rs`, new): copy into the user font library
  (`<data dir>/charted/fonts`), read the family name (`ttf-parser`), `Request::ListFonts`
  / `AddFont` / `RemoveFont`, `DataRequest::ReadFont` (bytes, like `ReadImage` at
  `api.rs:1034`). UI registers each as an injected `@font-face` with a data: URL (allowed
  by the CSP `font-src 'self' data:`, and html-to-image embeds stylesheet fonts, so
  agent screenshots see them).
- **Travels with the project:** on save, any library font the report uses (theme text
  classes or a visual's `fontFamily`) is copied to `<project>/fonts/`; on open, project
  fonts are registered too (and offered to add to the library).
- **Where fonts are set:** the theme editor's list (`ThemeEditor.tsx:31`) becomes
  bundled + yours + Power BI names; new `fontFamily` prop on title, slicer header, text
  box, data labels, card callout and table grid in `registry.ts`, with a font picker
  editor in `formatEditors.tsx`; `textTokens` also maps `callout.fontFace`.
  ECharts reads the visual's or theme's font instead of the root `--font-ui`
  (`echart.ts:69`, pass the element to `token`).
- **PBIR:** export/import `fontFamily` on titles, data labels and text-run `textStyle`.
- **Agent:** catalog gains `fontFamily`; `set_theme` custom accepts `fonts:{title,
  label,callout}`; `list_visual_types` (or describe_report) lists the available fonts;
  prompt line. The user's Helvetica request then works (Helvetica → fallback when not
  installed is reported in the tool result).
- Tests: Rust (`fonts.rs` add/list/family name, project copy on save), Vitest (font
  fallback map), e2e (pick a bundled font for a title; add a font file fixture and use
  it in a text box; reopen the project elsewhere and it still renders).

### Stage 4: the demo GIF — done (docs/media, bin/demo)

- `bin/demo` (new): starts `charted-headless serve` with the real HOME (so Claude Code
  is signed in) but throwaway docs/state dirs, and Vite; runs a Playwright script in
  `ui/demo/agent.demo.ts` with its own config (not part of `bin/test`), recording video
  at 1600×1000: open the sample, open the Agent pane, type one prompt (an overview
  dashboard: cards, trend, category split, region slicer, a markdown text box header in
  a bundled font), wait for the build, then click a slicer value to cross-filter.
- The script logs timestamps; ffmpeg speeds the waiting stretches (≈3×), then writes
  `docs/media/agent.gif` (1200 px wide, ~12 fps, palettegen, under 10 MB) and
  `docs/media/agent.mp4` (H.264, for LinkedIn). Real Claude Code (sonnet), ~$0.10–0.30
  per take; up to three takes, keep the best. Check frames visually before using.
- README: the GIF becomes the hero image above the screenshots. Send both files to the
  user.

### Stage 5: release v1.7.0 (user said go)

Same pattern as 1.6.0 (`2467912`, `83a43f4`, `c901696`):
1. Full `bin/heavy bin/test`; `bin/eval` once more (≈$0.20) to check the prompt edits.
2. Bump 1.7.0 in `Cargo.toml`, `ui/package.json`, `pkgbuild/{PKGBUILD,PKGBUILD-bin,
   local/PKGBUILD}`, Cargo.lock; README "what's new"; commit "Release 1.7.0: …".
3. Push `main` and tag `v1.7.0`; `.github/workflows/release.yml` builds the draft
   release with tarballs; check CI, then publish the draft with notes (M9 + this batch).
4. Fill the PKGBUILD checksums from SHA256SUMS; commit and push.
5. Update omarchy-pkgs PR #1879 to 1.7.0. Run `bin/aur-publish` only if
   `ssh -i ~/.ssh/aur aur@aur.archlinux.org help` shows the key is registered.
6. HANDOFF/PLAN/memory: v1.7.0 released.

### Verification (batch and M10)

- After each stage: `bin/heavy bin/test` (Rust, Vitest, Playwright) green, plus the new
  tests listed in each stage.
- Fonts and markdown: open the user's Harbor & Pine copy in the dev server. Check that
  the slicer shows its name once, then change the title fonts and a markdown text box.
  Take a screenshot with `screenshot_page` and confirm the fonts come through.
- GIF: inspect sample frames, the duration, and that the file is under 10 MB.
- Release: CI green on the tag, a downloadable tarball, the checksums match.
- M10: CI builds on all three platforms, then the user tries them on the Mac and in the
  Windows VM.

## M10: Mac and Windows (after v1.7.0)

The audit found 9 blockers, and nothing has been gated: there are no `cfg(unix)` or
`cfg(windows)` blocks in the codebase. This machine is aarch64 Linux, so macOS and
Windows are built and tested on CI runners (`macos-14`, `windows-latest`). The user can
run the builds on their Mac host and their Windows VM.

**M10.1: it compiles everywhere** (`cargo check` on all three platforms in CI first)
- In `src-tauri/Cargo.toml`, move gtk, webkit2gtk, glib, cairo and ashpd under
  `cfg(target_os="linux")`. Gate `set_prgname`, `screenshot.rs` and `pdf.rs`.
- Gate the agent control socket:
  - Unix keeps its Unix socket.
  - Windows uses loopback TCP on an ephemeral port, with the existing per-turn token.
    The address is written to the MCP config.
  - Make `charted mcp --socket` take that address.
- Stop: Unix keeps process groups. Windows uses a job object (`windows-sys`) or
  `taskkill /T /F`.
- Gate the Hyprland watcher. Copy the skill instead of symlinking it on Windows. Gate the
  0600 test.
- Add `.ico` and `.icns` icons (`bin/icons`). Rewrite `bin/wasm`'s staleness check in
  node, so `pnpm build` works on all three.

**M10.2: it runs properly**
- Paths:
  - State and data from `dirs` (`~/Library/Application Support`, `%APPDATA%`), and
    Documents from the Known Folder.
  - Use `dunce::canonicalize`.
  - In `steps.rs:637`, use `Path::is_absolute`. In `NewSource.tsx` and `FileMenu.tsx`,
    split paths on both `/` and `\`.
- Dialogs: tauri-plugin-dialog off Linux (the portal stays on Linux). Opening links and
  files: tauri-plugin-opener instead of `xdg-open`.
- Window chrome: native decorations off Linux (macOS uses `titleBarStyle: Overlay` with
  a drag region in the toolbar).
- Theme: follow the OS light or dark setting (`prefers-color-scheme` plus Tauri's theme
  event) when there is no Omarchy theme. Language falls back to the OS locale. Skip the
  `hyprctl` and `omarchy-*` lookups off Linux. Neutral wording ("Follow the system").
- Agents:
  - `on_path` honours PATHEXT, and `.cmd` shims run through `cmd /c` with
    `CREATE_NO_WINDOW`.
  - On macOS, import the login shell's PATH.
  - Open in terminal: Terminal.app through `osascript`; `wt.exe` falling back to
    `cmd /c start` on Windows.
- Keyring: add the `apple-native` and `windows-native` backends.
- Project lock: replace the `/proc` check with a cross-platform process check
  (`sysinfo`, or `kill(0)` and OpenProcess).
- Shortcuts:
  - Show ⌘ on macOS (`Kbd.tsx` and the help text).
  - Match on `e.code` for Alt combinations.
  - Tell Ctrl and Cmd apart on macOS.
- PDF export:
  - macOS: WKWebView `createPDF`.
  - Windows: WebView2 `PrintToPdf`.
  - Until then, fall back to `window.print()` on those platforms.
- DuckDB:
  - macOS: bundle `libduckdb.dylib` through `bundle.macOS.frameworks`, with
    `@executable_path/../Frameworks` as the rpath.
  - Windows: `duckdb.dll` next to the exe, through resources.

**M10.3: packaging and CI**
- `bundle.active` with `dmg`/`app` (universal) and `msi`/`nsis`, plus file associations
  for `.charted`, `.chartedz` and `.pbip`.
- `ci.yml`: add a macOS and a Windows matrix (Rust tests, and Playwright e2e on macOS).
  `release.yml` uploads the dmg and the msi.
- Windows e2e: make the webServer command a node script, not POSIX shell.
- Signing and notarisation wait on the user's Apple Developer and Windows code-signing
  decisions **(go-ahead)**. Unsigned builds go out as "preview" assets until then.
- The user checks the builds on their Mac and in their Windows VM; that is the visual
  checkpoint (`bin/checkpoint m10`).

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

## Decisions (2026-10-31)

1. **AUR and omarchy-pkgs: go.**
   - omarchy-pkgs PR omacom/omarchy-pkgs#1879 (`charted-bin`) is open; it waits for a
     maintainer's build approval.
   - AUR: the key `~/.ssh/aur.pub` has to be added to the AUR account, then
     `bin/aur-publish` publishes `charted` and `charted-bin`.
2. **GitHub Discussions: on.**
3. **Floating panes:** trial floating inside the window. Revisit OS windows only if the
   trial falls short.
4. **Layouts:** no per-project layouts. Each user has several named profiles (Standard,
   Report + model, Two report pages, …).
5. **M10 (Mac and Windows) comes after M9.**
