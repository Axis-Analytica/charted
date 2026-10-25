#!/usr/bin/env node
// Lists every translatable string in the UI (arguments of t() and tn()), and with
// --check verifies each catalog in i18n/ against them: no unknown keys, and reports
// missing translations. Used by CI and when adding a language.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const files = [];
const walk = (d) => {
  for (const f of readdirSync(d)) {
    const p = join(d, f);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(ts|tsx)$/.test(f) && !p.includes("/generated/") && !p.includes("/spike/")) files.push(p);
  }
};
walk(join(root, "ui/src"));

const strings = new Map();
const re = /\b(t|tn|msg)\(\s*"((?:[^"\\]|\\.)*)"/g;
for (const f of files) {
  const src = readFileSync(f, "utf8");
  for (const m of src.matchAll(re)) strings.set(JSON.parse(`"${m[2]}"`), m[1] === "tn" ? "tn" : "t");
}
// Shortcut labels and groups are translated where they are displayed.
for (const f of files) {
  const src = readFileSync(f, "utf8");
  for (const m of src.matchAll(/\blabel: "((?:[^"\\]|\\.)+)"/g)) strings.set(JSON.parse(`"${m[1]}"`), "t");
  for (const m of src.matchAll(/\bgroup: "(\w+)"/g)) strings.set(m[1], "t");
}

if (process.argv.includes("--check")) {
  let failed = false;
  for (const f of readdirSync(join(root, "i18n")).filter((f) => f.endsWith(".json"))) {
    const cat = JSON.parse(readFileSync(join(root, "i18n", f), "utf8"));
    const unknown = Object.keys(cat).filter((k) => !strings.has(k));
    const missing = [...strings.keys()].filter((k) => !(k in cat));
    if (unknown.length) {
      failed = true;
      console.error(`${f}: unknown keys:\n  ${unknown.join("\n  ")}`);
    }
    console.log(`${f}: ${strings.size - missing.length}/${strings.size} translated`);
  }
  process.exit(failed ? 1 : 0);
} else {
  for (const [s, kind] of [...strings].sort()) console.log(`${kind}\t${s}`);
}
