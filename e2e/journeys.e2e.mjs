// User journeys against the real renderer with a mocked Tauri IPC (TEST-17).
// Run with `pnpm test:e2e`. Each journey maps to an item of the release
// checklist in docs/release-checklist-e2e.md.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { invokes, startApp, withPage } from "./harness.mjs";

let app;

before(async () => {
  app = await startApp();
});

after(async () => {
  await app?.close();
});

async function skipWizard(page) {
  await page.getByRole("button", { name: "Skip setup" }).click();
  await page
    .getByRole("dialog")
    .waitFor({ state: "detached" })
    .catch(() => {});
}

async function unhandled(page) {
  return [...new Set(await page.evaluate(() => window.__E2E__.unhandled))];
}

function card(page, name) {
  // A provider card is the closest ancestor that also holds its action buttons.
  return page
    .getByText(name, { exact: true })
    .locator("xpath=ancestor::*[.//button[@title='Edit']][1]");
}

test("J1 boot: provider list renders with only mocked commands", async () => {
  await withPage(app, "J1-boot", async (page) => {
    await skipWizard(page);
    await card(page, "Claude Default").getByText("In Use").waitFor();
    await card(page, "Claude Custom")
      .getByRole("button", { name: "Enable" })
      .waitFor();
    assert.deepEqual(
      await unhandled(page),
      [],
      "every startup invoke has a handler",
    );
    const providers = await invokes(page, "get_providers");
    assert.ok(providers.some((c) => c.args?.app === "claude"));
  });
});

test("J2 switch: enabling a provider calls switch_provider and moves 'In Use'", async () => {
  await withPage(app, "J2-switch", async (page) => {
    await skipWizard(page);
    await card(page, "Claude Custom")
      .getByRole("button", { name: "Enable" })
      .click();
    await page.getByText("Switch successful!").waitFor();
    await card(page, "Claude Custom").getByText("In Use").waitFor();
    const calls = await invokes(page, "switch_provider");
    assert.deepEqual(
      calls.map((c) => c.args),
      [{ id: "claude-2", app: "claude" }],
    );
    // The tray menu is refreshed after a successful switch.
    assert.ok((await invokes(page, "update_tray_menu")).length >= 1);
  });
});

test("J3 apps: the app switcher loads that app's providers", async () => {
  await withPage(app, "J3-apps", async (page) => {
    await skipWizard(page);
    await page
      .locator("button", { hasText: /Codex$/ })
      .first()
      .click();
    await page.getByText("Codex Secondary", { exact: true }).waitFor();
    await page.getByText("Codex Default", { exact: true }).waitFor();
    const calls = await invokes(page, "get_providers");
    assert.ok(calls.some((c) => c.args?.app === "codex"));
  });
});

test("J4 switch failure: a backend rejection shows the localized error toast", async () => {
  await withPage(app, "J4-switch-failure", async (page) => {
    await skipWizard(page);
    await page.evaluate(() => {
      window.__E2E__.overrides.switch_provider = () => {
        // The exact shape AppError serializes to (tests/contracts/app_error.json).
        throw {
          code: "errors.config",
          message: "配置错误: live config is read-only",
          params: { detail: "live config is read-only" },
        };
      };
    });
    await card(page, "Claude Custom")
      .getByRole("button", { name: "Enable" })
      .click();
    await page.getByText("Switch failed", { exact: true }).waitFor();
    await page
      .getByText(/Configuration error: live config is read-only/)
      .waitFor();
    // Nothing changed.
    await card(page, "Claude Default").getByText("In Use").waitFor();
  });
});

test("J5 settings: the settings page opens and lists its sections", async () => {
  await withPage(app, "J5-settings", async (page) => {
    await skipWizard(page);
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("tab").first().waitFor();
    const tabs = await page.getByRole("tab").allInnerTexts();
    assert.ok(tabs.length >= 3, `settings tabs: ${tabs.join(", ")}`);
    assert.ok((await invokes(page, "get_settings")).length >= 1);
  });
});

test("J6 add provider: the add dialog opens with presets", async () => {
  await withPage(app, "J6-add-provider", async (page) => {
    await skipWizard(page);
    await page.getByRole("button", { name: "Add Provider" }).first().click();
    await page.getByRole("dialog", { name: "Add New Provider" }).waitFor();
    assert.ok((await invokes(page, "get_preset_catalog")).length >= 1);
  });
});
