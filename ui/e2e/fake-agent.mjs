// A stand-in for Claude Code in tests (CHARTED_AGENT_CMD="node fake-agent.mjs"): it reads
// the MCP config charted passes, starts `charted mcp`, calls charted's tools as a script
// chosen by a word in the request, and prints Claude Code's stream-json.
//   build — a page with a line chart, a donut and a slicer; then checks the numbers
//   slow   — adds two cards a second apart (to edit or stop mid-request)
//   peek   — lists a column's values (needs data access)
//   delete — removes the page "Agent e2e" (asks the user)
//   fail   — exits with an error
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const config = JSON.parse(flag("--mcp-config"));
const server = config.mcpServers.charted;
const session = flag("--resume") ?? flag("--session-id") ?? "fake-session";
const out = (o) => process.stdout.write(JSON.stringify(o) + "\n");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let prompt = "";
for await (const chunk of process.stdin) prompt += chunk;

out({ type: "system", subtype: "init", session_id: session, tools: [], mcp_servers: [{ name: "charted", status: "connected" }] });
if (/\bfail\b/i.test(prompt)) {
  process.stderr.write("fake agent: asked to fail\n");
  process.exit(3);
}

const mcp = spawn(server.command, server.args, { env: { ...process.env, ...server.env }, stdio: ["pipe", "pipe", "inherit"] });
const replies = new Map();
createInterface({ input: mcp.stdout }).on("line", (l) => {
  const m = JSON.parse(l);
  replies.get(m.id)?.(m);
});
let next = 1;
const rpc = (method, params) =>
  new Promise((resolve) => {
    const id = next++;
    replies.set(id, resolve);
    mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  });
const tool = async (name, args) => {
  out({ type: "assistant", message: { content: [{ type: "tool_use", id: `t${next}`, name: `mcp__charted__${name}`, input: args }] } });
  const r = await rpc("tools/call", { name, arguments: args });
  const text = r.result?.content?.[0]?.text ?? JSON.stringify(r.error);
  out({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: `t${next}`, content: text, is_error: !!r.result?.isError }] } });
  return { text, error: !!r.result?.isError };
};

await rpc("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "fake", version: "1" } });
let said = "Nothing to do.";
if (/\bbuild\b/i.test(prompt)) {
  await tool("describe_model", {});
  await tool("page", { action: "add", name: "Agent e2e" });
  await tool("add_visual", { page: "Agent e2e", type: "lineChart", title: "Sales by month", fields: { category: ["'Date'[YearMonth]"], values: ["[Total Sales]"] } });
  await tool("add_visual", { page: "Agent e2e", type: "donutChart", title: "Sales by category", fields: { legend: ["'Product'[Category]"], values: ["[Total Sales]"] } });
  await tool("add_visual", { page: "Agent e2e", type: "slicer", fields: { field: ["'Store'[Region]"] } });
  await tool("arrange_page", { page: "Agent e2e" });
  await tool("query_visual", { visual: "Sales by category" });
  said = "Built the page Agent e2e with sales by month, by category and a region slicer.";
} else if (/\bslow\b/i.test(prompt)) {
  await tool("add_visual", { page: "Page 1", type: "card", title: "Slow one", fields: { values: ["[Total Sales]"] } });
  await sleep(1500);
  await tool("add_visual", { page: "Page 1", type: "card", title: "Slow two", fields: { values: ["[Total Sales]"] } });
  await sleep(4000);
  said = "Added two cards.";
} else if (/\bpeek\b/i.test(prompt)) {
  const r = await tool("distinct_values", { field: "'Store'[Region]" });
  said = r.error ? "I can't see the values." : `The regions are ${r.text}`;
} else if (/\bdelete\b/i.test(prompt)) {
  const r = await tool("page", { action: "remove", page: "Agent e2e" });
  said = r.error ? "I left the page." : "Removed the page.";
}
out({ type: "assistant", message: { content: [{ type: "text", text: said }] } });
out({ type: "result", subtype: "success", is_error: false, result: said, session_id: session, total_cost_usd: 0.0123, num_turns: 3 });
mcp.stdin.end();
process.exit(0);
