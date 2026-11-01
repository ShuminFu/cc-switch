// User journeys against the real renderer with a mocked Tauri IPC (TEST-17).
// Run with `pnpm test:e2e`. Each journey maps to an item of the release
// checklist in docs/dev/release-checklist-e2e.md.
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

// ---------------------------------------------------------------------------
// Release-checklist journeys (PRODUCT-45, TEST-17). The checklist item each
// one covers is listed in docs/dev/release-checklist-e2e.md.
// ---------------------------------------------------------------------------

const waitForInvoke = (page, cmd) =>
  page.waitForFunction(
    (name) => window.__E2E__.calls.some((c) => c.cmd === name),
    cmd,
  );

test("J7 switch per app: Codex switches through its own app id", async () => {
  await withPage(app, "J7-switch-codex", async (page) => {
    await skipWizard(page);
    await page
      .locator("button", { hasText: /Codex$/ })
      .first()
      .click();
    await card(page, "Codex Secondary")
      .getByRole("button", { name: "Enable" })
      .click();
    await card(page, "Codex Secondary").getByText("In Use").waitFor();
    const calls = await invokes(page, "switch_provider");
    assert.deepEqual(
      calls.map((c) => c.args),
      [{ id: "codex-2", app: "codex" }],
    );
    // The Claude list was not touched by the Codex switch.
    await page
      .locator("button", { hasText: /Claude Code$/ })
      .first()
      .click();
    await card(page, "Claude Default").getByText("In Use").waitFor();
  });
});

test("J8 settings save: toggling a General setting persists it", async () => {
  await withPage(app, "J8-settings-save", async (page) => {
    await skipWizard(page);
    await page.getByRole("button", { name: "Settings" }).click();
    const toggle = page.getByRole("switch", {
      name: "Minimize to tray on close",
    });
    await toggle.waitFor();
    const before = await toggle.getAttribute("aria-checked");
    await toggle.click();
    await waitForInvoke(page, "save_settings");
    const saved = (await invokes(page, "save_settings")).at(-1).args.settings;
    assert.equal(saved.minimizeToTrayOnClose, before !== "true");
    // The page shows the saved value.
    await page.waitForFunction(
      (expected) =>
        document
          .querySelector('[aria-label="Minimize to tray on close"]')
          ?.getAttribute("aria-checked") === expected,
      before === "true" ? "false" : "true",
    );
  });
});

test("J9 export/import: SQL backup export and a confirmed import", async () => {
  await withPage(app, "J9-import-export", async (page) => {
    await skipWizard(page);
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("tab", { name: "Advanced" }).click();
    await page.getByText("Data Management", { exact: true }).click();

    await page.getByRole("button", { name: /Export SQL Backup/ }).click();
    await waitForInvoke(page, "export_config_to_file");
    const [exported] = await invokes(page, "export_config_to_file");
    assert.equal(exported.args.filePath, "/mock/export-settings.json");

    await page.getByRole("button", { name: /Select SQL File/ }).click();
    await page.getByRole("button", { name: /^Import/ }).click();
    // FE-UI-55: importing replaces everything, so it asks first.
    const confirm = page.getByRole("dialog", {
      name: "Replace all current data?",
    });
    await confirm.waitFor();
    assert.equal((await invokes(page, "import_config_from_file")).length, 0);
    await confirm.getByRole("button", { name: "Import" }).click();
    await waitForInvoke(page, "import_config_from_file");
    const [imported] = await invokes(page, "import_config_from_file");
    assert.equal(imported.args.filePath, "/mock/import-settings.json");
  });
});

test("J10 failover toggle: enabling failover for a taken-over app", async () => {
  await withPage(
    app,
    "J10-failover-toggle",
    async (page) => {
      await skipWizard(page);
      const toggle = page.getByRole("switch", { name: "Claude failover" });
      await toggle.waitFor();
      assert.equal(await toggle.getAttribute("aria-checked"), "false");
      await toggle.click();
      await waitForInvoke(page, "set_auto_failover_enabled");
      const [call] = await invokes(page, "set_auto_failover_enabled");
      assert.deepEqual(call.args, { appType: "claude", enabled: true });
      await page.waitForFunction(
        () =>
          document
            .querySelector('[aria-label="Claude failover"]')
            ?.getAttribute("aria-checked") === "true",
      );
    },
    {
      setup: () => {
        let failover = false;
        window.__E2E_SETUP__ = {
          overrides: {
            // The header toggle is opt-in (Settings > Routing) and needs the
            // app's live config taken over by the local route.
            get_settings: async (_args, next) => ({
              ...(await next()),
              enableFailoverToggle: true,
              failoverConfirmed: true,
            }),
            get_proxy_takeover_status: { claude: true, codex: false },
            get_proxy_overview: async (_args, next) => ({
              ...(await next()),
              takeover: { claude: true, codex: false },
            }),
            get_auto_failover_enabled: () => failover,
            set_auto_failover_enabled: (args) => {
              failover = args.enabled;
              return null;
            },
          },
        };
      },
    },
  );
});

test("J11 deep link: a provider link previews, and imports only on confirm", async () => {
  await withPage(app, "J11-deeplink", async (page) => {
    await skipWizard(page);
    await page.evaluate(() =>
      window.__E2E__.emit("deeplink-import", {
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Linked Relay",
        endpoint: "https://relay.example.com",
        apiKey: "sk-e2e-placeholder",
        nonce: "e2e-nonce",
      }),
    );
    const dialog = page.getByRole("dialog");
    await dialog.getByText("Linked Relay").first().waitFor();
    assert.equal(
      (await invokes(page, "import_from_deeplink_unified")).length,
      0,
      "nothing is imported before the user confirms",
    );
    await dialog.getByRole("button", { name: /^Import$/ }).click();
    await page.getByText("Import successful").waitFor();
    const [call] = await invokes(page, "import_from_deeplink_unified");
    assert.equal(call.args.request.nonce, "e2e-nonce");
    // Switching to the imported provider is a separate opt-in (SEC-02).
    assert.notEqual(call.args.request.enabled, true);
  });
});
