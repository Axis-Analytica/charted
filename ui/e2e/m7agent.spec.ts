// M7, the in-app agent: requests in the Agent pane go to a scripted stand-in for Claude
// Code (e2e/fake-agent.mjs), which builds through charted's own tools. Its changes land
// live, the canvas follows them, and a request undoes in one step.
import { expect, type Page, test } from "@playwright/test";

const shots = process.env.CHARTED_SHOTS;

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
  const steps = turn.locator(".agent-step");
  await expect(steps).toHaveCount(5);
  await expect(steps.nth(1)).toContainText('Added line chart "Sales by month"');
  await expect(turn.locator(".agent-text")).toContainText("Built the page Agent e2e");
  await expect(turn.locator(".agent-cost")).toContainText("$0.01");
  if (shots) await page.screenshot({ path: `${shots}/m7-agent.png` });
  // A step selects what it made.
  await steps.nth(2).locator(".agent-step-text").click();
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
