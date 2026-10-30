import { beforeEach, describe, expect, it, vi } from "vitest";
import { emitTauriEvent } from "../msw/tauriMocks";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import {
  settingsApi,
  TOOL_LIFECYCLE_OUTPUT_EVENT,
  type ToolLifecycleOutputLine,
} from "@/lib/api/settings";

describe("tool lifecycle api", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("passes runId and timeoutSecs to run_tool_lifecycle_action", async () => {
    await settingsApi.runToolLifecycleAction(["claude"], "update", undefined, {
      runId: "run-1",
      timeoutSecs: 120,
    });

    expect(invokeMock).toHaveBeenCalledWith("run_tool_lifecycle_action", {
      tools: ["claude"],
      action: "update",
      wslShellByTool: undefined,
      runId: "run-1",
      timeoutSecs: 120,
    });
  });

  it("cancels a run by id", async () => {
    invokeMock.mockResolvedValue(true);
    await expect(settingsApi.cancelToolLifecycleAction("run-1")).resolves.toBe(
      true,
    );
    expect(invokeMock).toHaveBeenCalledWith("cancel_tool_lifecycle_action", {
      runId: "run-1",
    });
  });

  it("delivers streamed output lines until unlistened", async () => {
    const received: ToolLifecycleOutputLine[] = [];
    const unlisten = await settingsApi.onToolLifecycleOutput((line) =>
      received.push(line),
    );
    const line: ToolLifecycleOutputLine = {
      runId: "run-1",
      stream: "stdout",
      line: "added 1 package",
    };
    emitTauriEvent(TOOL_LIFECYCLE_OUTPUT_EVENT, line);
    unlisten();
    emitTauriEvent(TOOL_LIFECYCLE_OUTPUT_EVENT, { ...line, line: "late" });

    expect(received).toEqual([line]);
  });
});
