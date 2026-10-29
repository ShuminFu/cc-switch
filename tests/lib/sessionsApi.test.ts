import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { sessionsApi } from "@/lib/api/sessions";

describe("sessionsApi.launchTerminal", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(true);
  });

  it("sends only the session identity, never a shell command or cwd", async () => {
    await sessionsApi.launchTerminal({
      providerId: "claude",
      sessionId: "abc;rm -rf ~",
    });

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("launch_session_terminal", {
      providerId: "claude",
      sessionId: "abc;rm -rf ~",
    });
    const [, args] = invokeMock.mock.calls[0];
    expect(Object.keys(args as object).sort()).toEqual([
      "providerId",
      "sessionId",
    ]);
  });
});
