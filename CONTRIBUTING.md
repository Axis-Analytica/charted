# Contributing to charted

Thanks for helping. charted is a local-first BI tool for Omarchy. Bug reports, Power BI
files that don't import, ideas, and code are all welcome.

- **Bugs and import problems:** [open an issue](https://github.com/Axis-Analytica/charted/issues/new/choose).
  For an import problem, the import report and a small sample project help most.
- **Ideas, questions, what you love or hate:** [Discussions](https://github.com/Axis-Analytica/charted/discussions).
- **Code:** open a pull request against `main`. For anything bigger than a fix, start a
  discussion first, so we agree on the shape before you build it.

Using a coding agent? Point it at [AGENTS.md](AGENTS.md); it has the same rules in the
form an agent needs.

## Setup

```sh
bin/setup   # Rust, pnpm, wasm-bindgen and the UI's dependencies, via mise
. bin/env   # in each new shell: puts the toolchain on PATH
bin/dev     # the app with hot reload
```

On a shared folder without file events (a VM's host folder), run with `CHARTED_POLL=1`.

## Tests

`bin/test` runs everything CI runs: formatting, clippy, the Rust tests (including the
DAX tests against a reference implementation), the WebAssembly check, TypeScript, the i18n
catalogs, Vitest and the Playwright end-to-end tests. Run it before you open a pull
request.

Builds and tests are memory-hungry. Run them through `bin/heavy` (as `bin/test` does), so
that on a small machine they queue instead of getting the terminal killed:
`bin/heavy cargo test -p charted-core`.

Focused runs:
- **Rust:** `bin/heavy cargo test -p <crate> [--test <file>] [name]`.
- **End-to-end:** `cd ui && CHROMIUM_PATH=$(command -v chromium) ../bin/heavy pnpm exec playwright test <spec>`.
  The specs share one sample project, so a spec that changes it must undo the change.

## How the code is laid out

| Path | What it is |
|---|---|
| `crates/model` | The semantic model, report types, changes and undo history |
| `crates/dax` | The DAX parser, binder, language service and catalog |
| `crates/compiler` | DAX and Power Query steps compiled to DuckDB SQL |
| `crates/core` | Projects, sessions, the request API, Power BI import |
| `crates/tmdl`, `crates/ossie` | Power BI (TMDL, PBIR) and Apache Ossie formats |
| `crates/agent` | The in-app agent's tools, control socket and runner |
| `crates/cli` | The command line, `charted mcp` and the development server |
| `src-tauri` | The desktop shell |
| `ui/` | The React UI |

[docs/HANDOFF.md](docs/HANDOFF.md) has the architecture and the current state, and
[docs/PLAN.md](docs/PLAN.md) has what's next.

## Conventions

- **Changes to the model go through `Change` and one commit.** That gives every edit
  undo for free; don't mutate the model anywhere else.
- **TypeScript bindings are generated.** After changing a Rust type the UI uses, run
  `bin/heavy cargo test -p <crate>` to rewrite `ui/src/bridge/generated/`. Don't edit
  those files by hand.
- **User-facing strings** go through `t()`, `tn()` or `msg()`. Add their Danish and
  Arabic translations to `i18n/*.json` at the end of each file (don't re-sort), and check
  with `node tools/i18n-strings.mjs --check`.
- **The UI isn't run through a formatter.** Match the surrounding style; lines are
  hand-wrapped at about 200 columns. Rust uses `cargo fmt`.
- **Write commit messages that say what changed and why**, in plain sentences.
- **No telemetry, ever.** The network is used only for what the user asks for.

## Licence

charted is Apache-2.0. By contributing you agree that your contribution is licensed the
same way.
