// Release smoke (CCS-201): first run -> add a preset -> switch and check
// ~/.claude/settings.json -> proxy takeover on -> restore.
import fs from "node:fs";
import path from "node:path";

const home = process.env.CC_SWITCH_E2E_HOME;
const claudeSettingsPath = path.join(home, ".claude", "settings.json");

const PRESET_NAME = "DeepSeek";
const PRESET_BASE_URL = "https://api.deepseek.com/anthropic";
const API_KEY = "sk-cc-switch-e2e-smoke";

function readClaudeEnv() {
  try {
    return JSON.parse(fs.readFileSync(claudeSettingsPath, "utf8")).env ?? {};
  } catch {
    return null;
  }
}

async function waitForClaudeEnv(predicate, timeoutMsg) {
  let last;
  await browser.waitUntil(
    () => {
      last = readClaudeEnv();
      return last != null && predicate(last);
    },
    { timeout: 20_000, interval: 250, timeoutMsg },
  );
  return last;
}

const providerCard = (name) =>
  $(`[data-testid="provider-card"][data-provider-name="${name}"]`);

// The provider's main action button ("Enable" / "In Use"). Card actions only
// become visible on hover.
async function mainAction(name) {
  const card = await providerCard(name);
  await card.waitForExist();
  await card.moveTo();
  const action = await card.$('[data-testid="provider-main-action"]');
  await action.waitForDisplayed();
  return action;
}

async function switchTo(name) {
  const action = await mainAction(name);
  await action.waitForEnabled();
  await action.click();
  await expect(action).toBeDisabled({ wait: 20_000 });
}

describe("CC Switch release smoke", () => {
  it("shows the first-run notice with the seeded official provider", async () => {
    const confirm = await $('[data-testid="first-run-notice-confirm"]');
    await confirm.waitForDisplayed({ timeout: 60_000 });
    await expect(providerCard("Claude Official")).toBeExisting();
    expect(fs.existsSync(path.join(home, ".cc-switch", "cc-switch.db"))).toBe(
      true,
    );

    await confirm.click();
    await confirm.waitForExist({ reverse: true });
  });

  it("adds a provider from a preset", async () => {
    await $('[data-testid="add-provider"]').click();
    // The provider form explains the common config snippet once per install.
    const notice = await $('[data-testid="confirm-dialog-confirm"]');
    await notice.waitForDisplayed();
    await notice.click();
    await notice.waitForExist({ reverse: true });

    const preset = await $(`button[data-preset-name="${PRESET_NAME}"]`);
    await preset.waitForClickable();
    await preset.click();

    const apiKey = await $("#apiKey");
    await apiKey.waitForDisplayed();
    await apiKey.setValue(API_KEY);

    await $('[data-testid="add-provider-submit"]').click();
    await providerCard(PRESET_NAME).waitForExist();

    // The official preset is seeded but not current, so the first provider
    // added becomes current and is written to the live config. Wait for the
    // list to show that too before switching, so a late refetch from the add
    // cannot overwrite the next switch in the UI.
    await waitForClaudeEnv(
      (env) => env.ANTHROPIC_BASE_URL === PRESET_BASE_URL,
      `${claudeSettingsPath} was not written for the first provider`,
    );
    await expect(await mainAction(PRESET_NAME)).toBeDisabled({ wait: 20_000 });
  });

  it("switches providers and rewrites ~/.claude/settings.json", async () => {
    // The new provider is already current: switch to the official provider
    // and back.
    await switchTo("Claude Official");
    await waitForClaudeEnv(
      (env) =>
        env.ANTHROPIC_BASE_URL === undefined &&
        env.ANTHROPIC_AUTH_TOKEN === undefined,
      `${claudeSettingsPath} was not switched to Claude Official`,
    );

    await switchTo(PRESET_NAME);
    await waitForClaudeEnv(
      (env) =>
        env.ANTHROPIC_BASE_URL === PRESET_BASE_URL &&
        env.ANTHROPIC_AUTH_TOKEN === API_KEY,
      `${claudeSettingsPath} was not switched to ${PRESET_NAME}`,
    );
  });

  it("takes over Claude with the local proxy", async () => {
    const takeover = await $('[data-testid="proxy-takeover-switch"]');
    await takeover.waitForClickable();
    await expect(takeover).toHaveAttribute("aria-checked", "false");
    await takeover.click();

    await expect(takeover).toHaveAttribute("aria-checked", "true", {
      wait: 30_000,
    });
    const env = await waitForClaudeEnv(
      (env) => /^http:\/\/127\.0\.0\.1:\d+/.test(env.ANTHROPIC_BASE_URL ?? ""),
      `${claudeSettingsPath} was not pointed at the local proxy`,
    );
    // The real key stays in the database; the live file must not carry it.
    expect(env.ANTHROPIC_AUTH_TOKEN).not.toBe(API_KEY);
  });

  it("restores the provider config when takeover is turned off", async () => {
    const takeover = await $('[data-testid="proxy-takeover-switch"]');
    await takeover.waitForClickable({ timeout: 30_000 });
    await takeover.click();

    await expect(takeover).toHaveAttribute("aria-checked", "false", {
      wait: 30_000,
    });
    await waitForClaudeEnv(
      (env) =>
        env.ANTHROPIC_BASE_URL === PRESET_BASE_URL &&
        env.ANTHROPIC_AUTH_TOKEN === API_KEY,
      `${claudeSettingsPath} was not restored to ${PRESET_NAME}`,
    );
  });
});
