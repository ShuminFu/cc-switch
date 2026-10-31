import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EnvConflictBanner } from "@/app/EnvConflictBanner";
import { AppLoadingScreen } from "@/components/AppLoadingScreen";
import { mockInvoke } from "../msw/invokeRecorder";

describe("startup env-conflict scan (PERF-33)", () => {
  it("does not repeat the per-app scan the all-apps scan already covered", async () => {
    const api = mockInvoke({ check_env_conflicts: [] });
    const { rerender } = render(<EnvConflictBanner activeApp="claude" />);

    // claude / codex / gemini from the startup scan, nothing more.
    await waitFor(() =>
      expect(api.callsTo("check_env_conflicts")).toHaveLength(3),
    );

    rerender(<EnvConflictBanner activeApp="codex" />);
    await waitFor(() =>
      expect(api.callsTo("check_env_conflicts")).toHaveLength(4),
    );
    expect(api.callsTo("check_env_conflicts")[3].args).toEqual({
      app: "codex",
    });
  });

  it("still checks an initial app outside the all-apps scan", async () => {
    const api = mockInvoke({ check_env_conflicts: [] });
    render(<EnvConflictBanner activeApp="opencode" />);
    await waitFor(() =>
      expect(api.callsTo("check_env_conflicts")).toHaveLength(4),
    );
  });
});

describe("AppLoadingScreen", () => {
  it("renders an accessible busy indicator", () => {
    render(<AppLoadingScreen />);
    expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true");
  });
});
