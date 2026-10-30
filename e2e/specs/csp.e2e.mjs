// CSP and icon rendering at runtime (CCS-226, follow-up of CCS-077/CCS-078):
// the policy from src-tauri/tauri.conf.json must block arbitrary connections
// but still let models.dev, https images and the bundled provider icons load,
// and the main views must not trip it.
//
// Negative control: put `https: http:` back into `connect-src` in
// src-tauri/tauri.conf.json and rebuild; the policy and fetch steps then fail.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const artifactsDir = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "artifacts",
);

// A provider preset whose icon is a bundled image (iconUrls in
// src/icons/extracted/index.ts), so the provider list renders an <img>.
const IMAGE_ICON_PRESET = "Shengsuanyun";
const API_KEY = "sk-cc-switch-e2e-csp";

// Violation events are dispatched asynchronously after the blocked request.
const VIOLATION_SETTLE_MS = 1_000;

const providerCard = (name) =>
  $(`[data-testid="provider-card"][data-provider-name="${name}"]`);

function installViolationCollector() {
  return browser.execute(() => {
    if (window.__cspViolations) return;
    window.__cspViolations = [];
    document.addEventListener("securitypolicyviolation", (event) => {
      window.__cspViolations.push({
        blockedURI: event.blockedURI,
        violatedDirective: event.violatedDirective,
        effectiveDirective: event.effectiveDirective,
      });
    });
  });
}

const violationCount = () =>
  browser.execute(() => window.__cspViolations.length);

// Violations recorded since `from` (an index into window.__cspViolations).
async function violationsSince(from) {
  await browser.pause(VIOLATION_SETTLE_MS);
  return browser.execute((start) => window.__cspViolations.slice(start), from);
}

const directiveOf = (violation) =>
  violation.effectiveDirective || violation.violatedDirective.split(" ")[0];

// Runs `fetch(url, init)` in the page; resolves to "ok <status>" or the error.
function pageFetch(url, init = {}) {
  return browser.executeAsync(
    (target, options, done) => {
      fetch(target, options).then(
        (response) => done(`ok ${response.status}`),
        (error) => done(`error ${String(error)}`),
      );
    },
    url,
    init,
  );
}

// Adds an <img> to the page and resolves to "load" or "error".
function pageImage(src) {
  return browser.executeAsync((url, done) => {
    const img = document.createElement("img");
    img.dataset.cspProbe = "true";
    img.style.position = "fixed";
    img.style.left = "-9999px";
    const timer = setTimeout(() => done("timeout"), 15_000);
    img.onload = () => {
      clearTimeout(timer);
      done("load");
    };
    img.onerror = () => {
      clearTimeout(timer);
      done("error");
    };
    img.src = url;
    document.body.appendChild(img);
  }, src);
}

async function clickByTitle(title) {
  const button = await $(`button[title="${title}"]`);
  await button.waitForClickable();
  await button.click();
}

async function goBack() {
  const back = await $('button[aria-label="Back"]');
  await back.waitForClickable();
  await back.click();
  await $('[data-testid="add-provider"]').waitForDisplayed();
}

describe("Content security policy at runtime", () => {
  it("serves the tightened policy", async () => {
    const settings = await $('button[title="Settings"]');
    await settings.waitForDisplayed({ timeout: 60_000 });
    // Only shown when this spec runs on a fresh HOME.
    const notice = await $('[data-testid="first-run-notice-confirm"]');
    if (await notice.isDisplayed()) {
      await notice.click();
      await notice.waitForExist({ reverse: true });
    }
    await installViolationCollector();

    // Tauri serves the policy as a response header of the app protocol.
    const policy = await browser.executeAsync((done) => {
      const meta = document.querySelector(
        'meta[http-equiv="Content-Security-Policy"]',
      );
      fetch(location.href).then(
        (response) =>
          done(
            response.headers.get("content-security-policy") ??
              meta?.content ??
              "",
          ),
        () => done(meta?.content ?? ""),
      );
    });
    const directives = Object.fromEntries(
      policy
        .split(";")
        .map((part) => part.trim().split(/\s+/))
        .filter(([name]) => name)
        .map(([name, ...sources]) => [name, sources]),
    );
    console.log(
      `[e2e] CSP connect-src: ${directives["connect-src"]?.join(" ")}; img-src: ${directives["img-src"]?.join(" ")}`,
    );
    expect(directives["connect-src"]).toBeDefined();
    expect(directives["connect-src"]).toContain("https://models.dev");
    expect(directives["connect-src"]).not.toContain("https:");
    expect(directives["connect-src"]).not.toContain("http:");
    expect(directives["connect-src"]).not.toContain("*");
  });

  it("blocks an arbitrary fetch with a connect-src violation", async () => {
    const from = await violationCount();
    const result = await pageFetch("https://example.com/");
    const violations = await violationsSince(from);
    console.log(
      `[e2e] fetch example.com: ${result}; ${JSON.stringify(violations)}`,
    );

    expect(result).toMatch(/^error /);
    // The violation proves the policy blocked it, not the (offline) network.
    expect(
      violations.some(
        (v) =>
          directiveOf(v) === "connect-src" &&
          v.blockedURI.includes("example.com"),
      ),
    ).toBe(true);
  });

  it("lets the models.dev catalogue through", async () => {
    const from = await violationCount();
    // Without network access this fails, which is fine: only the policy is
    // under test.
    const result = await pageFetch("https://models.dev/api.json", {
      method: "HEAD",
    });
    const violations = await violationsSince(from);
    console.log(
      `[e2e] HEAD models.dev: ${result}; ${JSON.stringify(violations)}`,
    );
    expect(violations).toEqual([]);
  });

  it("allows https images and blocks plain http ones", async () => {
    let from = await violationCount();
    const httpsResult = await pageImage(
      "https://avatars.githubusercontent.com/u/9919?v=4",
    );
    let violations = await violationsSince(from);
    console.log(
      `[e2e] https image: ${httpsResult}; ${JSON.stringify(violations)}`,
    );
    expect(violations.filter((v) => directiveOf(v) === "img-src")).toEqual([]);

    from = await violationCount();
    const httpResult = await pageImage("http://example.com/x.png");
    violations = await violationsSince(from);
    console.log(
      `[e2e] http image: ${httpResult}; ${JSON.stringify(violations)}`,
    );
    expect(httpResult).toBe("error");
    expect(
      violations.some(
        (v) =>
          directiveOf(v) === "img-src" && v.blockedURI.includes("example.com"),
      ),
    ).toBe(true);

    await browser.execute(() =>
      document.querySelectorAll("[data-csp-probe]").forEach((n) => n.remove()),
    );
  });

  it("renders provider icons without the asset protocol", async () => {
    // Add a provider with an image icon next to the inline-SVG ones.
    if (!(await providerCard(IMAGE_ICON_PRESET).isExisting())) {
      await $('[data-testid="add-provider"]').click();
      // Only shown the first time the provider form opens on this HOME.
      const formNotice = await $('[data-testid="confirm-dialog-confirm"]');
      if (await formNotice.isDisplayed()) {
        await formNotice.click();
        await formNotice.waitForExist({ reverse: true });
      }
      const preset = await $(`button[data-preset-name="${IMAGE_ICON_PRESET}"]`);
      await preset.waitForClickable();
      await preset.click();
      const apiKey = await $("#apiKey");
      await apiKey.waitForDisplayed();
      await browser.waitUntil(
        async () => {
          await apiKey.clearValue();
          await apiKey.setValue(API_KEY);
          return (await apiKey.getValue()) === API_KEY;
        },
        { timeout: 20_000, interval: 250, timeoutMsg: "API key was not typed" },
      );
      await $('[data-testid="add-provider-submit"]').click();
      await providerCard(IMAGE_ICON_PRESET).waitForExist();
      // Let the form panel close so the screenshot shows the list.
      await apiKey.waitForExist({ reverse: true });
      await browser.pause(500);
    }

    let icons;
    await browser
      .waitUntil(
        async () => {
          icons = await browser.execute(() => {
            const cards = [
              ...document.querySelectorAll('[data-testid="provider-card"]'),
            ];
            return {
              cards: cards.length,
              images: cards.flatMap((card) =>
                [...card.querySelectorAll("img")].map((img) => ({
                  card: card.dataset.providerName,
                  src: img.getAttribute("src") ?? "",
                  complete: img.complete,
                  naturalWidth: img.naturalWidth,
                })),
              ),
              svgs: cards.flatMap((card) =>
                // ProviderIcon renders inline SVG inside a span titled with
                // the provider name.
                [...card.querySelectorAll("span[title] > svg")]
                  .filter(
                    (svg) =>
                      svg.parentElement.title === card.dataset.providerName,
                  )
                  .map((svg) => {
                    const box = svg.getBoundingClientRect();
                    return {
                      card: card.dataset.providerName,
                      width: box.width,
                      height: box.height,
                    };
                  }),
              ),
            };
          });
          return (
            icons.images.length > 0 &&
            icons.images.every((img) => img.complete && img.naturalWidth > 0)
          );
        },
        {
          timeout: 20_000,
          interval: 250,
          timeoutMsg: "provider icon images did not load",
        },
      )
      .catch((error) => {
        console.log(`[e2e] provider icons: ${JSON.stringify(icons)}`);
        throw error;
      });
    console.log(`[e2e] provider icons: ${JSON.stringify(icons)}`);

    expect(icons.images.some((img) => img.card === IMAGE_ICON_PRESET)).toBe(
      true,
    );
    for (const img of icons.images) {
      expect(img.src.startsWith("asset:")).toBe(false);
      expect(img.src).not.toContain("asset.localhost");
    }
    // Claude Official carries an inline-SVG brand icon.
    expect(icons.svgs.some((svg) => svg.card === "Claude Official")).toBe(true);
    for (const svg of icons.svgs) {
      expect(svg.width).toBeGreaterThan(0);
      expect(svg.height).toBeGreaterThan(0);
    }

    fs.mkdirSync(artifactsDir, { recursive: true });
    await browser.saveScreenshot(path.join(artifactsDir, "csp-icons.png"));
  });

  it("records no violation while visiting the main views", async () => {
    const from = await violationCount();

    await clickByTitle("Settings");
    for (const tab of [
      "General",
      "Routing",
      "Auth",
      "Advanced",
      "Usage Statistics",
      "About",
    ]) {
      const trigger = await $(
        `//button[@role="tab"][normalize-space(.)="${tab}"]`,
      );
      await trigger.waitForClickable();
      await trigger.click();
      await expect(trigger).toHaveAttribute("data-state", "active");
    }
    // Settings > About shows the bundled app icon.
    const appIcon = await $('img[alt="CC Switch"]');
    await appIcon.waitForDisplayed();
    await browser.waitUntil(
      () =>
        browser.execute((img) => img.complete && img.naturalWidth > 0, appIcon),
      { timeout: 10_000, timeoutMsg: "About app icon did not load" },
    );
    expect((await appIcon.getAttribute("src")).startsWith("asset:")).toBe(
      false,
    );
    await goBack();

    for (const view of [
      "Skills",
      "Prompts",
      "Session Manager",
      "MCP Management",
    ]) {
      await clickByTitle(view);
      await $('button[aria-label="Back"]').waitForDisplayed();
      await browser.pause(500);
      await goBack();
    }

    const violations = await violationsSince(from);
    console.log(
      `[e2e] violations while navigating: ${JSON.stringify(violations)}`,
    );
    expect(violations).toEqual([]);
  });
});
