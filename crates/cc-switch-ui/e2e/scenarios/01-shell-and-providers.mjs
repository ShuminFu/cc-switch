// Shell + provider list basics.
export async function run({ page, expect }) {
  const header = page.locator("header");
  await header.getByText("Claude Code").waitFor({ timeout: 20000 });
  const switcher = await header.innerText();
  expect(/Claude Code[\s\S]*Codex[\s\S]*Gemini/.test(switcher), "app switcher lists the visible apps in order");
  expect(!switcher.includes("Claude Desktop"), "hidden app (visibleApps) is not listed");
  expect(!switcher.includes("OpenClaw"), "hidden OpenClaw is not listed");
  // Header actions are translated (English because the mocked settings say so).
  expect((await header.locator("button[title='Settings']").count()) === 1, "settings button rendered with English title");
  await header.locator("button[title='Settings']").click();
  await page.locator("header h1", { hasText: "Settings" }).waitFor({ timeout: 5000 });
  expect(await page.evaluate(() => localStorage.getItem("cc-switch-last-view")) === "settings", "view persisted under the React storage key");
  await header.locator("button[title='Back']").click();
  await header.getByText("Claude Code").waitFor({ timeout: 5000 });
  // Provider list for Claude: sorted by sortIndex, current badge, partner badge.
  const cards = page.locator("main ul > li");
  await cards.first().waitFor({ timeout: 10000 });
  const names = await cards.locator("span.font-semibold").allInnerTexts();
  expect(names[0] === "Kimi For Coding" && names[1] === "Anthropic Official", `providers sorted by sortIndex (${names.join(" | ")})`);
  expect((await cards.nth(1).innerText()).includes("Currently Using"), "current provider badge on the active provider");
  expect((await cards.nth(0).innerText()).includes("Official Partner"), "partner badge from meta.isPartner");
  expect((await cards.nth(0).locator("img, span[data-icon], span[title='Kimi For Coding'] svg").count()) > 0, "provider icon rendered");
  await cards.nth(0).getByRole("button", { name: "Enable" }).click();
  await cards.nth(0).getByText("Currently Using").waitFor({ timeout: 5000 });
  const switched = await page.evaluate(() => window.__TAURI_MOCK__.calls.filter((c) => c.cmd === "switch_provider").map((c) => c.args));
  expect(switched.length === 1 && switched[0].id === "p-kimi" && switched[0].app === "claude", `switch_provider invoked with id/app (${JSON.stringify(switched)})`);
  await page.locator("main input[type=search]").fill("team");
  expect((await cards.count()) === 1, "search filters by notes");
  await page.locator("main input[type=search]").fill("");
  await header.getByText("Codex").click();
  expect(await page.evaluate(() => localStorage.getItem("cc-switch-last-app")) === "codex", "active app persisted under the React storage key");
  await page.getByText("No providers added yet").waitFor({ timeout: 5000 });
  const htmlClass = await page.evaluate(() => document.documentElement.className);
  expect(/\b(light|dark)\b/.test(htmlClass), `theme class applied to <html> (${htmlClass})`);
  const calls = await page.evaluate(() => window.__TAURI_MOCK__.calls.map((c) => c.cmd));
  expect(calls.includes("get_settings"), `get_settings invoked (calls: ${calls.join(",")})`);
  expect(calls.includes("set_window_theme"), "native window theme synced");
}
