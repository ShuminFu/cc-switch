import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";

const toastErrorMock = vi.fn();
const deleteMutateMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getDefaultCostMultiplier: vi.fn().mockResolvedValue("1"),
    getPricingModelSource: vi.fn().mockResolvedValue("response"),
    setDefaultCostMultiplier: vi.fn(),
    setPricingModelSource: vi.fn(),
  },
}));

vi.mock("@/lib/query/usage", () => ({
  useModelPricing: () => ({
    data: [
      {
        modelId: "gpt-x",
        displayName: "GPT X",
        inputCostPerMillion: "1",
        outputCostPerMillion: "2",
        cacheReadCostPerMillion: "0",
        cacheCreationCostPerMillion: "0",
      },
    ],
    isLoading: false,
    error: null,
  }),
  useDeleteModelPricing: () => ({
    mutate: deleteMutateMock,
    isPending: false,
  }),
}));

describe("PricingConfigPanel", () => {
  beforeEach(() => {
    toastErrorMock.mockReset();
    deleteMutateMock.mockReset();
  });

  it("shows an error toast and keeps the dialog open when delete fails", async () => {
    deleteMutateMock.mockImplementation(
      (_id: string, opts: { onError?: (e: unknown) => void }) => {
        opts.onError?.("database is locked");
      },
    );

    render(<PricingConfigPanel />);

    fireEvent.click(screen.getByTitle("common.delete"));
    const confirm = await screen.findByRole("button", {
      name: "common.delete",
    });
    fireEvent.click(confirm);

    expect(deleteMutateMock).toHaveBeenCalledWith("gpt-x", expect.anything());
    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("common.error", {
        description: "database is locked",
      }),
    );
    expect(screen.getByText("usage.deleteConfirmTitle")).toBeInTheDocument();
  });
});
