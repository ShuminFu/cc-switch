import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { ModelsDevPickerDialog } from "@/components/usage/ModelsDevPickerDialog";
import { createTestQueryClient } from "../utils/testQueryClient";
import { server } from "../msw/server";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const catalog = {
  openai: {
    name: "OpenAI",
    models: {
      "gpt-4o": {
        name: "GPT-4o",
        release_date: "2024-05-13",
        cost: { input: 2.5, output: 10 },
      },
      "gpt-4.1": {
        name: "GPT-4.1",
        release_date: "2025-04-14",
        cost: { input: 2, output: 8 },
      },
    },
  },
};

describe("ModelsDevPickerDialog keyboard support (FE-UI-44)", () => {
  it("renders rows as buttons that arrow keys move between", async () => {
    server.use(
      http.get("https://models.dev/api.json", () => HttpResponse.json(catalog)),
    );
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ModelsDevPickerDialog open onClose={vi.fn()} onImported={vi.fn()} />
      </QueryClientProvider>,
    );

    const first = await screen.findByRole("button", { name: /GPT-4\.1/ });
    const second = screen.getByRole("button", { name: /GPT-4o/ });
    expect(first.tagName).toBe("BUTTON");

    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(document.activeElement).toBe(second);
    fireEvent.keyDown(second, { key: "ArrowUp" });
    expect(document.activeElement).toBe(first);

    expect(first).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(first);
    expect(first).toHaveAttribute("aria-pressed", "true");
  });
});
