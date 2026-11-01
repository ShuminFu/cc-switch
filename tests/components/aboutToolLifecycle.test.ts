import { describe, expect, it } from "vitest";
import {
  classifyRunResult,
  summarizeRun,
  TOOL_NAMES,
} from "@/components/settings/about/toolLifecycle";
import {
  HERMES_WINDOWS_INSTALL_COMMAND,
  HERMES_WINDOWS_INSTALL_SCRIPT,
  decodePowershellEncodedCommand,
  installScriptSources,
  oneClickInstallCommands,
  powershellEncodedCommand,
} from "@/components/settings/about/installCommands";

describe("classifyRunResult (FE-UI-19)", () => {
  it("reports an update whose version did not move", () => {
    expect(
      classifyRunResult({
        action: "update",
        previousVersion: "1.0.0",
        previousLatestVersion: "1.2.0",
        refreshed: { version: "1.0.0", latest_version: "1.2.0", error: null },
      }),
    ).toEqual({ kind: "versionUnchanged", version: "1.0.0", latest: "1.2.0" });
  });

  it("reports a tool that installed but cannot run", () => {
    expect(
      classifyRunResult({
        action: "install",
        previousVersion: null,
        previousLatestVersion: null,
        refreshed: {
          version: null,
          latest_version: "2.0.0",
          error: " needs node 22 ",
        },
      }),
    ).toEqual({ kind: "notRunnable", detail: "needs node 22" });
    expect(
      classifyRunResult({
        action: "install",
        previousVersion: null,
        previousLatestVersion: null,
        refreshed: undefined,
      }),
    ).toEqual({ kind: "notRunnable", detail: null });
  });

  it("treats a moved or up-to-date version as success", () => {
    expect(
      classifyRunResult({
        action: "update",
        previousVersion: "1.0.0",
        previousLatestVersion: "1.2.0",
        refreshed: { version: "1.2.0", latest_version: "1.2.0", error: null },
      }),
    ).toEqual({ kind: "success" });
    expect(
      classifyRunResult({
        action: "install",
        previousVersion: "1.0.0",
        previousLatestVersion: "1.2.0",
        refreshed: { version: "1.0.0", latest_version: "1.2.0", error: null },
      }),
    ).toEqual({ kind: "success" });
  });
});

describe("summarizeRun", () => {
  it("summarises success, soft-only, hard and partial batches", () => {
    expect(summarizeRun([], 3, true)).toEqual({
      level: "success",
      succeeded: 3,
    });

    const unchanged = summarizeRun(
      [
        {
          toolName: "codex",
          detail: "still 1.0",
          soft: true,
          kind: "versionUnchanged",
        },
      ],
      0,
      false,
    );
    expect(unchanged).toEqual({
      level: "versionUnchanged",
      description: "still 1.0",
    });

    const notRunnable = summarizeRun(
      [
        {
          toolName: "codex",
          detail: "x",
          soft: true,
          kind: "versionUnchanged",
        },
        { toolName: "openclaw", detail: "y", soft: true, kind: "notRunnable" },
      ],
      0,
      true,
    );
    expect(notRunnable.level).toBe("notRunnable");

    const failed = summarizeRun(
      [{ toolName: "gemini", detail: "line1\nnpm ERR! boom", soft: false }],
      0,
      true,
    );
    expect(failed).toEqual({
      level: "failed",
      description: "Gemini CLI: npm ERR! boom",
    });

    const partial = summarizeRun(
      [{ toolName: "hermes", detail: "denied", soft: false }],
      2,
      true,
    );
    expect(partial).toEqual({
      level: "partial",
      succeeded: 2,
      failed: 1,
      description: "Hermes: denied",
    });
  });

  it("covers all seven tools", () => {
    expect(TOOL_NAMES).toHaveLength(7);
  });
});

describe("install command snippets (FE-UI-45)", () => {
  it("round-trips the PowerShell encoded command", () => {
    const encoded = powershellEncodedCommand(HERMES_WINDOWS_INSTALL_SCRIPT);
    expect(decodePowershellEncodedCommand(encoded)).toBe(
      HERMES_WINDOWS_INSTALL_SCRIPT,
    );
    expect(HERMES_WINDOWS_INSTALL_COMMAND).toContain(encoded);
    expect(decodePowershellEncodedCommand("%%%")).toBeNull();
  });

  it("shows the decoded Hermes script and lists every remote script URL", () => {
    expect(oneClickInstallCommands(true)).toContain(
      `runs: ${HERMES_WINDOWS_INSTALL_SCRIPT}`,
    );
    for (const windows of [true, false]) {
      const text = oneClickInstallCommands(windows);
      for (const { url } of installScriptSources(windows)) {
        expect(text).toContain(windows ? url.replace(".ps1", ".ps1") : url);
      }
    }
  });
});
