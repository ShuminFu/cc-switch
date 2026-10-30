import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import type { DriftField, DriftReport } from "@/shared/bindings";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { driftApi } from "@/lib/api/drift";
import { buildDriftPrompt } from "@/hooks/useLiveConfigDrift";

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key} ${JSON.stringify(options)}` : key) as unknown as TFunction;

function report(overrides: Partial<DriftReport>): DriftReport {
  return {
    app: "claude",
    takeoverActive: false,
    leftoverPlaceholder: false,
    fields: [],
    files: ["/home/u/.claude/settings.json"],
    fingerprint: "f",
    ignored: false,
    ...overrides,
  };
}

function field(path: string[], foldable: boolean): DriftField {
  return {
    file: "/home/u/.claude/settings.json",
    path,
    change: "added",
    foldable,
  };
}

describe("buildDriftPrompt", () => {
  it("offers repair for a leftover takeover placeholder", () => {
    const prompt = buildDriftPrompt(
      report({
        leftoverPlaceholder: true,
        fields: [field(["env", "ANTHROPIC_AUTH_TOKEN"], false)],
      }),
      t,
    );
    expect(prompt.message).toContain("notifications.liveDriftPlaceholder");
    expect(prompt.primary?.resolution).toEqual({ kind: "reconcile" });
  });

  it("offers folding only the foldable external fields", () => {
    const prompt = buildDriftPrompt(
      report({
        fields: [field(["hooks"], true), field(["env", "MY_API_KEY"], false)],
      }),
      t,
    );
    expect(prompt.message).toContain("hooks, env.MY_API_KEY");
    expect(prompt.primary?.resolution).toEqual({
      kind: "fold",
      paths: [["hooks"]],
    });
  });

  it("has no primary action when nothing can be folded", () => {
    const prompt = buildDriftPrompt(
      report({ fields: [field(["env", "MY_API_KEY"], false)] }),
      t,
    );
    expect(prompt.primary).toBeUndefined();
  });
});

describe("driftApi", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("invokes the drift commands with the app id and resolution", async () => {
    invokeMock.mockResolvedValue(report({}));

    await driftApi.detect("claude");
    await driftApi.resolve("claude", { kind: "fold", paths: [["hooks"]] });
    await driftApi.detectAll();

    expect(invokeMock.mock.calls).toEqual([
      ["get_live_config_drift", { app: "claude" }],
      [
        "resolve_live_config_drift",
        { app: "claude", resolution: { kind: "fold", paths: [["hooks"]] } },
      ],
      ["get_all_live_config_drift"],
    ]);
  });
});
