// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { isLinux, isMac, isWindows } from "@/lib/platform";

/** Shared-layer coverage for small src/lib modules (FE-ARCH-15). */

describe("resolveManagedAccountId", () => {
  it("returns the bound account for the matching auth provider", () => {
    expect(
      resolveManagedAccountId(
        {
          authBinding: {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "acc-1",
          },
        },
        "codex_oauth",
      ),
    ).toBe("acc-1");
  });

  it("ignores a binding for another provider or source", () => {
    const meta = {
      authBinding: {
        source: "provider_config" as const,
        authProvider: "codex_oauth",
        accountId: "acc-1",
      },
    };
    expect(resolveManagedAccountId(meta, "codex_oauth")).toBeNull();
    expect(resolveManagedAccountId(undefined, "codex_oauth")).toBeNull();
  });

  it("falls back to the legacy githubAccountId for Copilot", () => {
    expect(
      resolveManagedAccountId({ githubAccountId: "gh-7" }, "github_copilot"),
    ).toBe("gh-7");
    expect(resolveManagedAccountId({}, "github_copilot")).toBeNull();
  });
});

describe("platform detection", () => {
  const setUa = (userAgent: string, platform = "") => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(userAgent);
    vi.spyOn(navigator, "platform", "get").mockReturnValue(platform);
  };
  afterEach(() => vi.restoreAllMocks());

  it("recognises macOS, Windows and Linux user agents", () => {
    setUa("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)", "MacIntel");
    expect([isMac(), isWindows(), isLinux()]).toEqual([true, false, false]);

    setUa("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32");
    expect([isMac(), isWindows(), isLinux()]).toEqual([false, true, false]);

    setUa("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64");
    expect([isMac(), isWindows(), isLinux()]).toEqual([false, false, true]);
  });

  it("does not treat Android as desktop Linux", () => {
    setUa("Mozilla/5.0 (Linux; Android 14)", "Linux armv8l");
    expect(isLinux()).toBe(false);
  });
});
