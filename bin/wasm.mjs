#!/usr/bin/env node
// Builds the DAX editor's language service (crates/wasm) into ui/src/dax/pkg, skipping
// the build when nothing it depends on has changed. Node, so it runs on every platform;
// bin/wasm wraps it in bin/heavy on Linux.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "ui/src/dax/pkg");
const stamp = join(out, ".stamp");

function newest(dir) {
  let t = 0;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) t = Math.max(t, newest(p));
    else if (e.name.endsWith(".rs") || e.name === "Cargo.toml") t = Math.max(t, Math.floor(statSync(p).mtimeMs / 1000));
  }
  return t;
}

const latest = String(Math.max(...["crates/wasm", "crates/dax", "crates/model"].map((d) => newest(join(root, d)))));
if (existsSync(stamp) && readFileSync(stamp, "utf8").trim() === latest && existsSync(join(out, "charted_wasm_bg.wasm"))) process.exit(0);

const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit", shell: process.platform === "win32" });
run("cargo", ["build", "-q", "--release", "--target", "wasm32-unknown-unknown", "-p", "charted-wasm"]);
const target = process.env.CARGO_TARGET_DIR || join(root, "target");
run("wasm-bindgen", ["--target", "web", "--out-dir", out, join(target, "wasm32-unknown-unknown/release/charted_wasm.wasm")]);
writeFileSync(stamp, `${latest}\n`);
