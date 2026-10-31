#!/usr/bin/env node
// bin/demo-cut.mjs CLIPS NAME TARGET FONT DIR: prints the ffmpeg filter graph that cuts
// one clip out of the demo recording (bin/demo). Each stretch of clips.json is
//   {from, to, speed: number | "auto", zoom?: {at: [x, y], from, to}, caption?}
// "auto" squeezes the stretch to about TARGET seconds; zoom eases the scale from `from`
// to `to` around the point `at` (in recording pixels) over the stretch's first second;
// captions fade in and out over the stretch, and an "auto" stretch says how sped up it is.
// Caption text goes to files in DIR (drawtext's textfile, so nothing needs escaping).
import { readFileSync, writeFileSync } from "node:fs";

const [file, name, target, font, dir] = process.argv.slice(2);
const { size = [1600, 1000], [name]: parts } = JSON.parse(readFileSync(file, "utf8"));
const [W, H] = size;
const RAMP = 0.9;

let at = 0;
const cuts = [];
const texts = [];
parts.forEach((p, i) => {
  const speed = p.speed === "auto" ? Math.max(1, (p.to - p.from) / Number(target)) : p.speed;
  const length = (p.to - p.from) / speed;
  let f = `[0:v]trim=${p.from.toFixed(2)}:${p.to.toFixed(2)},setpts=(PTS-STARTPTS)/${speed.toFixed(3)}`;
  if (p.zoom) {
    const { at: [x, y], from, to } = p.zoom;
    // Smoothstep from `from` to `to`; scale and crop both evaluate it per frame.
    const z = `(${from}+(${to - from})*st(0,clip(t/${RAMP},0,1))*ld(0)*(3-2*ld(0)))`;
    f += `,scale=w='${W}*${z}':h='${H}*${z}':eval=frame:flags=lanczos`;
    f += `,crop=${W}:${H}:x='clip(${x}*${z}-${W / 2},0,${W}*${z}-${W})':y='clip(${y}*${z}-${H / 2},0,${H}*${z}-${H})'`;
  }
  cuts.push(`${f},setsar=1[p${i}]`);
  const end = at + length;
  if (p.caption) texts.push({ text: p.caption, from: at, to: end, big: true });
  if (p.speed === "auto" && speed > 1.5) texts.push({ text: `${Math.round(speed)}× speed`, from: at, to: end, big: false });
  at = end;
});

const draw = texts.map((c, i) => {
  const path = `${dir}/caption-${name}-${i}.txt`;
  writeFileSync(path, c.text);
  const fade = `clip(min((t-${c.from.toFixed(2)})/0.35,(${c.to.toFixed(2)}-t)/0.35),0,1)`;
  const where = c.big ? `x=(w-tw)/2:y=h-th-110:fontsize=46:boxborderw=22|30` : `x=w-tw-36:y=30:fontsize=24:boxborderw=10|14`;
  return `drawtext=fontfile='${font}':textfile='${path}':fontcolor=white:box=1:boxcolor=0x111111@0.82:${where}:alpha='${fade}':enable='between(t,${c.from.toFixed(2)},${c.to.toFixed(2)})'`;
});

console.log(
  `${cuts.join(";")};${parts.map((_, i) => `[p${i}]`).join("")}concat=n=${parts.length}:v=1${draw.length ? `,${draw.join(",")}` : ""}[v]`,
);
