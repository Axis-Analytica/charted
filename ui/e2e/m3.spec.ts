import { expect, type Page, test } from "@playwright/test";

const tmp = process.env.CHARTED_E2E_TMP ?? "/tmp/charted-e2e";
const shots = process.env.CHARTED_SHOTS;

async function openSample(page: Page) {
  await page.goto("/");
  const explore = page.getByRole("button", { name: "Explore the sample" });
  await expect(explore.or(page.locator(".shell"))).toBeVisible();
  if (await explore.isVisible()) await explore.click();
  await expect(page.locator(".report-view")).toBeVisible({ timeout: 30_000 });
}

async function shot(page: Page, name: string) {
  if (shots) await page.screenshot({ path: `${shots}/${name}.png` });
}

test.setTimeout(120_000);

test("model view: the star, a new relationship, Both refused as ambiguous", async ({ page }) => {
  page.on("console", (m) => console.log("CONSOLE", m.type(), m.text()));
  page.on("pageerror", (e) => console.log("PAGEERROR", e.message));
  await openSample(page);
  await page.keyboard.press("Control+3");
  await expect(page.locator(".model-card")).toHaveCount(6);
  await expect(page.locator(".model-edge")).toHaveCount(6);
  await expect(page.locator(".model-edge.is-inactive")).toHaveCount(1);
  await shot(page, "01-model-star");

  // Drag Targets[StoreKey] onto Store[StoreKey]: the dialog checks the keys and
  // suggests many-to-one.
  const col = (table: string, column: string) => page.locator(`.model-card[aria-label="${table}"] .model-col`, { hasText: new RegExp(`^${column}$`) });
  await col("Targets", "StoreKey").dragTo(col("Store", "StoreKey"));
  const dialog = page.getByRole("dialog", { name: "New relationship" });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".rel-check")).toContainText("every key has a match");
  await expect(dialog.getByLabel("Cardinality")).toHaveValue("manyToOne");
  await shot(page, "02-new-relationship");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(page.locator(".model-edge")).toHaveCount(7);

  // Filtering both ways would give Sales two paths to Store (direct, and through
  // Date → Targets → Store): refused, as in Power BI.
  await page.locator(".model-rel", { hasText: "Targets[StoreKey] → Store[StoreKey]" }).dblclick();
  const edit = page.getByRole("dialog", { name: "Edit relationship" });
  await edit.getByLabel("Cross filter direction").selectOption("Both");
  await edit.getByRole("button", { name: "Save" }).click();
  await expect(edit.getByRole("alert")).toContainText("two active paths");
  await shot(page, "03-both-refused");
  await edit.getByRole("button", { name: "Cancel" }).click();
});
