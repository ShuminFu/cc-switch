import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import PromptPanel from "@/components/prompts/PromptPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/useTauriEvent", () => ({ useTauriEvent: vi.fn() }));

const toggleCalls: Array<[string, boolean]> = [];

vi.mock("@/hooks/usePromptActions", () => ({
  usePromptActions: () => ({
    prompts: {
      p1: {
        id: "p1",
        name: "First",
        content: "",
        enabled: false,
        createdAt: 1,
        updatedAt: 1,
      },
    },
    loading: false,
    reload: vi.fn(),
    savePrompt: vi.fn(),
    deletePrompt: vi.fn(),
    // Plain function (not vi.fn): spies attach their own .then() to returned
    // promises, which would mask an unhandled rejection.
    toggleEnabled: (id: string, enabled: boolean) => {
      toggleCalls.push([id, enabled]);
      return Promise.reject(new Error("enable failed"));
    },
  }),
}));

describe("PromptPanel", () => {
  it("does not leak an unhandled rejection when toggling fails", async () => {
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);

    try {
      render(<PromptPanel open={true} onOpenChange={vi.fn()} appId="claude" />);
      fireEvent.click(screen.getByRole("switch"));

      await waitFor(() => expect(toggleCalls).toEqual([["p1", true]]));
      await new Promise((r) => setTimeout(r, 0));
      expect(unhandled).not.toHaveBeenCalled();
    } finally {
      process.off("unhandledRejection", unhandled);
    }
  });
});
