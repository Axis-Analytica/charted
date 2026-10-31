// M7, the in-app agent: requests in the Agent pane go to a scripted stand-in for Claude
// Code (e2e/fake-agent.mjs), which builds through charted's own tools. Its changes land
// live, the canvas follows them, and a request undoes in one step.
import { expect, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

// Pane layout is kept by the core across tests: close the panes a test opened.
test.afterEach(async ({ page }) => {
  for (const name of ["Bookmarks pane", "Selection pane", "Sync slicers pane", "Agent pane"]) {
    const b = page.getByRole("button", { name, exact: true });
    if ((await b.count()) && (await b.first().getAttribute("aria-pressed")) === "true") await b.first().click();
  }
});

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (!(await explore.isVisible()) && !(await page.locator(".topbar").textContent())?.includes("Harbor & Pine")) {
    await page.getByRole("button", { name: "File" }).click();
    await page.getByRole("menuitem", { name: "Close project" }).click();
    await expect(explore).toBeVisible();
  }
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
}

const pane = (page: Page) => page.locator(".agent-pane");
const turns = (page: Page) => pane(page).locator(".agent-turn");

async function openPane(page: Page) {
  if (!(await pane(page).isVisible())) await page.keyboard.press("Control+i");
  await expect(pane(page).locator(".agent-picker")).toContainText("Claude Code");
}

async function newChat(page: Page) {
  const button = pane(page).getByRole("button", { name: "New chat" });
  if (await button.isEnabled()) await button.click();
  await expect(turns(page)).toHaveCount(0);
}

async function ask(page: Page, text: string) {
  const before = await turns(page).count();
  await pane(page).getByRole("textbox", { name: "Ask the agent" }).fill(text);
  await pane(page).getByRole("button", { name: "Send", exact: true }).click();
  const agree = page.getByRole("dialog").getByRole("button", { name: "Agree and send" });
  await expect(agree.or(turns(page).nth(before))).toBeVisible();
  if (await agree.isVisible()) {
    await expect(page.getByRole("dialog")).toContainText("names and types");
    await agree.click();
    // The request leaves the box once it's sent.
    await expect(pane(page).getByRole("textbox", { name: "Ask the agent" })).toHaveValue("");
  }
  await expect(turns(page)).toHaveCount(before + 1);
  return turns(page).nth(before);
}

const done = (turn: ReturnType<Page["locator"]>) => expect(turn.locator(".agent-working")).toHaveCount(0, { timeout: 30_000 });

test("the agent builds a page live, and the request undoes in one step", async ({ page }) => {
  await openSample(page);
  await openPane(page);
  await newChat(page);
  const turn = await ask(page, "build me a page");
  await done(turn);
  // The canvas followed the agent to its page.
  await expect(page.getByRole("tab", { name: "Agent e2e" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("visual")).toHaveCount(3);
  // A slicer's title is its header, shown once (not "Region Region").
  const slicer = page.getByTestId("visual").filter({ has: page.locator(".slicer") });
  await expect(slicer.locator(".slicer-title")).toHaveText("Region");
  await expect(slicer.locator(".visual-title")).toHaveCount(0);
  const steps = turn.locator(".agent-step");
  await expect(steps).toHaveCount(6);
  await expect(steps.nth(1)).toContainText('Added line chart "Sales by month"');
  // The misspelt try it then got right is muted, not red.
  await expect(steps.nth(2)).toHaveClass(/is-retried/);
  await expect(turn.locator(".agent-step.is-failed")).toHaveCount(0);
  await expect(turn.locator(".agent-text")).toContainText("Built the page Agent e2e");
  // Replies are markdown.
  await expect(turn.locator(".agent-text strong")).toHaveText("Agent e2e");
  await expect(turn.locator(".agent-cost")).toContainText("$0.01");
  if (shots) await page.screenshot({ path: `${shots}/m7-agent.png` });
  // A step selects what it made.
  await steps.nth(3).locator(".agent-step-text").click();
  await expect(page.locator(".visual.is-selected")).toHaveCount(1);
  // One undo takes the whole request back.
  await expect(page.getByRole("button", { name: "Undo Agent: build me a page" })).toBeVisible();
  await turn.getByRole("button", { name: "Undo this request" }).click();
  await expect(page.getByRole("tab", { name: "Agent e2e" })).toHaveCount(0);
});

test("Stop ends a request, and an edit made meanwhile keeps its own undo step", async ({ page }) => {
  await openSample(page);
  await openPane(page);
  await newChat(page);
  const cards = page.getByTestId("visual").filter({ hasText: /Slow (one|two)/ });
  const before = await page.getByTestId("visual").count();
  const start = (await page.locator(".page-tab.is-on").textContent())!.trim();
  // An edit while the agent works: its request no longer undoes as one.
  let turn = await ask(page, "slow cards");
  await expect(turn.locator(".agent-step")).toHaveCount(1);
  await page.getByRole("button", { name: "New page" }).click();
  await done(turn);
  await expect(turn.getByRole("button", { name: "Undo this request" })).toBeDisabled();
  await expect(turn.locator(".agent-note")).toBeVisible();
  for (let i = 0; i < 3; i++) await page.keyboard.press("Control+z");
  await page.getByRole("tab", { name: start, exact: true }).click();
  await expect(page.getByTestId("visual")).toHaveCount(before);
  // Stop.
  turn = await ask(page, "slow cards");
  await expect(turn.locator(".agent-step")).toHaveCount(1);
  await pane(page).getByRole("button", { name: "Stop" }).click();
  await expect(turn.locator(".agent-turn-foot")).toContainText("Stopped");
  await expect(cards).toHaveCount(1);
  await turn.getByRole("button", { name: "Undo this request" }).click();
  await expect(page.getByTestId("visual")).toHaveCount(before);
});

test("the agent sees values only with data access, and asks before deleting a page", async ({ page }) => {
  await openSample(page);
  await openPane(page);
  await newChat(page);
  let turn = await ask(page, "peek at the regions");
  await done(turn);
  await expect(turn.locator(".agent-step.is-failed")).toContainText("Needs data access");
  await pane(page).getByRole("button", { name: "Let the agent see data" }).click();
  turn = await ask(page, "peek at the regions");
  await done(turn);
  await expect(turn.locator(".agent-text")).toContainText("values in all");
  await pane(page).getByRole("button", { name: /The agent can see data/ }).click();

  const build = await ask(page, "build me a page");
  await done(build);
  turn = await ask(page, "delete it");
  await expect(turn.locator(".agent-confirm")).toContainText('Delete the page "Agent e2e"');
  await turn.getByRole("button", { name: "Skip" }).click();
  await done(turn);
  await expect(page.getByRole("tab", { name: "Agent e2e" })).toHaveCount(1);
  await expect(turn.locator(".agent-answer")).toHaveText("Skipped");
  turn = await ask(page, "delete it");
  await turn.getByRole("button", { name: "Allow" }).click();
  await done(turn);
  await expect(page.getByRole("tab", { name: "Agent e2e" })).toHaveCount(0);
  await turn.getByRole("button", { name: "Undo this request" }).click();
  await expect(page.getByRole("tab", { name: "Agent e2e" })).toHaveCount(1);
  await build.getByRole("button", { name: "Undo this request" }).click();
  await expect(page.getByRole("tab", { name: "Agent e2e" })).toHaveCount(0);

  // The conversation is kept with the project; New chat clears it.
  await page.reload();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
  await openPane(page);
  await expect(turns(page)).not.toHaveCount(0);
  await pane(page).getByRole("button", { name: "New chat" }).click();
  await expect(turns(page)).toHaveCount(0);
});

test("query changes are drafted, and applied only when the user says", async ({ page }) => {
  await openSample(page);
  await openPane(page);
  await newChat(page);
  const card = pane(page).getByRole("region", { name: "Query changes" });
  let turn = await ask(page, "draft a sort");
  await done(turn);
  await expect(card).toContainText("Store");
  await card.getByRole("button", { name: "Discard" }).click();
  await expect(card).toHaveCount(0);
  turn = await ask(page, "draft a sort");
  await done(turn);
  await card.getByRole("button", { name: "Apply" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Apply" }).click();
  await expect(card).toHaveCount(0);
  // Applying starts undo afresh.
  await expect(page.getByRole("button", { name: "Undo", exact: true })).toBeDisabled();
  turn = await ask(page, "undraft it");
  await done(turn);
  await card.getByRole("button", { name: "Apply" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Apply" }).click();
  await expect(card).toHaveCount(0);
});

test("project instructions go with every request, and the agent can add one when allowed", async ({ page }) => {
  await openSample(page);
  await openPane(page);
  await newChat(page);
  await pane(page).getByRole("button", { name: "Project instructions", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("textbox", { name: "Project instructions" }).fill("- Titles in sentence case");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toHaveCount(0);
  const turn = await ask(page, "what are the rules");
  await turn.getByRole("button", { name: "Allow" }).click();
  await done(turn);
  await expect(turn.locator(".agent-text")).toContainText("I was given: - Titles in sentence case");
  await expect(turn.locator(".agent-step")).toContainText('Remembered "Money in thousands"');
  // Both are in the file now; clear them for the specs after this one.
  await pane(page).getByRole("button", { name: "Project instructions (the agent follows them)" }).click();
  const box = dialog.getByRole("textbox", { name: "Project instructions" });
  await expect(box).toHaveValue("- Titles in sentence case\n- Money in thousands\n");
  await box.fill("");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(pane(page).getByRole("button", { name: "Project instructions", exact: true })).toBeVisible();
});

test("the agent sees a picture of the page only with data access", async ({ page }) => {
  await openSample(page);
  await openPane(page);
  await newChat(page);
  const blind = await ask(page, "look at this page");
  await done(blind);
  await expect(blind.locator(".agent-step.is-failed")).toContainText("Needs data access");
  await expect(blind.locator(".agent-text")).toContainText("I can't see the page.");
  const data = pane(page).getByRole("button", { name: "Let the agent see data" });
  await data.click();
  const seen = await ask(page, "look at this page");
  await done(seen);
  await expect(seen.locator(".agent-step")).toContainText("Looked at page");
  await expect(seen.locator(".agent-text")).toContainText(/I saw the page: a PNG of \d+×\d+ pixels\./);
  await pane(page).getByRole("button", { name: /The agent can see data/ }).click();
});
