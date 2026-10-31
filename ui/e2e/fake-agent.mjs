// A stand-in for Claude Code in tests (CHARTED_AGENT_CMD="node fake-agent.mjs"): it reads
// the MCP config charted passes, starts `charted mcp`, calls charted's tools as a script
// chosen by a word in the request, and prints Claude Code's stream-json.
//   build — a page with a line chart, a donut (after one misspelt try) and a slicer; then
//           checks the numbers
//   slow   — adds two cards a second apart (to edit or stop mid-request)
//   peek   — lists a column's values (needs data access)
//   delete — removes the page "Agent e2e" (asks the user)
//   draft  — drafts sorting the Store query (a step that changes nothing else)
//   undraft — drafts taking that step out again
//   layout — floats the Fields pane, puts the model beside the report, hides Align left
//   rules  — says which project instructions it was given, and remembers one more
//   look   — takes a picture of the page the user is on (needs data access)
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
// The page the user is on, from the context charted adds to the request.
const here = () => /on the page ".*?" \(id ([^)]+)\)/.exec(prompt)?.[1];
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
let lastImage = null;
const tool = async (name, args) => {
  out({ type: "assistant", message: { content: [{ type: "tool_use", id: `t${next}`, name: `mcp__charted__${name}`, input: args }] } });
  const r = await rpc("tools/call", { name, arguments: args });
  const text = r.result?.content?.[0]?.text ?? JSON.stringify(r.error);
  lastImage = r.result?.content?.find((c) => c.type === "image") ?? null;
  out({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: `t${next}`, content: text, is_error: !!r.result?.isError }] } });
  return { text, error: !!r.result?.isError };
};

await rpc("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "fake", version: "1" } });
let said = "Nothing to do.";
if (/\bbuild\b/i.test(prompt)) {
  await tool("describe_model", {});
  await tool("page", { action: "add", name: "Agent e2e" });
  await tool("add_visual", { page: "Agent e2e", type: "lineChart", title: "Sales by month", fields: { category: ["'Date'[YearMonth]"], values: ["[Total Sales]"] } });
  // A misspelt field first, put right on the next try (the pane shows it as retried).
  await tool("add_visual", { page: "Agent e2e", type: "donutChart", title: "Sales by category", fields: { legend: ["'Product'[Categry]"], values: ["[Total Sales]"] } });
  await tool("add_visual", { page: "Agent e2e", type: "donutChart", title: "Sales by category", fields: { legend: ["'Product'[Category]"], values: ["[Total Sales]"] } });
  await tool("add_visual", { page: "Agent e2e", type: "slicer", title: "Region", fields: { field: ["'Store'[Region]"] } });
  await tool("arrange_page", { page: "Agent e2e" });
  await tool("query_visual", { visual: "Sales by category" });
  said = "Built the page Agent e2e with sales by month, by category and a region slicer.";
} else if (/\bslow\b/i.test(prompt)) {
  await tool("add_visual", { page: here(), type: "card", title: "Slow one", fields: { values: ["[Total Sales]"] } });
  await sleep(1500);
  await tool("add_visual", { page: here(), type: "card", title: "Slow two", fields: { values: ["[Total Sales]"] } });
  await sleep(4000);
  said = "Added two cards.";
} else if (/\bpeek\b/i.test(prompt)) {
  const r = await tool("distinct_values", { field: "'Store'[Region]" });
  said = r.error ? "I can't see the values." : `The regions are ${r.text}`;
} else if (/\blayout\b/i.test(prompt)) {
  await tool("describe_workspace", {});
  await tool("set_pane", { pane: "fields", place: "float", x: 60, y: 80 });
  await tool("split_view", { view: "model" });
  await tool("list_commands", {});
  await tool("hide_commands", { ids: ["align-left"] });
  said = "Floated the fields pane, put the model beside the report and hid Align left.";
} else if (/\bundraft\b/i.test(prompt)) {
  const steps = (await tool("describe_queries", { query: "Store" })).text.split("\n").filter((l) => /^ {2}\d+\. .*"sortRows"/.test(l));
  const index = Number(/^ {2}(\d+)\./.exec(steps.at(-1) ?? "")?.[1]);
  await tool("draft_query", { action: "remove_step", query: "Store", index });
  said = "Drafted taking the sort out; press Apply.";
} else if (/\bdraft\b/i.test(prompt)) {
  await tool("draft_query", { action: "add_step", query: "Store", step: { op: "sortRows", columns: [["Region", false]] } });
  said = "Drafted sorting Store by region; press Apply in the Agent pane.";
} else if (/\blook\b/i.test(prompt)) {
  const r = await tool("screenshot_page", { page: here() });
  const png = lastImage && lastImage.mimeType === "image/png" ? Buffer.from(lastImage.data, "base64") : null;
  said = r.error ? "I can't see the page." : png && png.subarray(1, 4).toString() === "PNG" ? `I saw the page: a PNG of ${png.readUInt32BE(16)}×${png.readUInt32BE(20)} pixels.` : "I got no picture.";
} else if (/\brules\b/i.test(prompt)) {
  const given = /The project's instructions[^\n]*\n([\s\S]*)$/.exec(prompt)?.[1]?.trim();
  await tool("remember", { instruction: "Money in thousands" });
  said = given ? `I was given: ${given.split("\n")[0]}` : "I was given no instructions.";
} else if (/\bdelete\b/i.test(prompt)) {
  const r = await tool("page", { action: "remove", page: "Agent e2e" });
  said = r.error ? "I left the page." : "Removed the page.";
}
out({ type: "assistant", message: { content: [{ type: "text", text: said }] } });
out({ type: "result", subtype: "success", is_error: false, result: said, session_id: session, total_cost_usd: 0.0123, num_turns: 3 });
mcp.stdin.end();
process.exit(0);
