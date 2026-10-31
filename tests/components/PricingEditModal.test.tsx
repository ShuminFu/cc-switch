import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PricingEditModal } from "@/components/usage/PricingEditModal";
import type { ModelPricing } from "@/types/usage";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: string | ({ defaultValue?: string } & Record<string, unknown>),
    ) => {
      if (typeof options === "string") return options;
      const text = options?.defaultValue ?? key;
      return text.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
        String(options?.[name] ?? ""),
      );
    },
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const mutateAsync = vi.hoisted(() => vi.fn());
vi.mock("@/lib/query/usage", () => ({
  useUpdateModelPricing: () => ({
    mutateAsync,
    isPending: false,
  }),
  useModelPricing: () => ({
    data: [
      {
        modelId: "gpt-4o",
        displayName: "GPT-4o",
        inputCostPerMillion: "2.5",
        outputCostPerMillion: "10",
        cacheReadCostPerMillion: "0",
        cacheCreationCostPerMillion: "0",
      },
    ],
  }),
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    children,
    footer,
  }: {
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) => (
    <div>
      {children}
      {footer}
    </div>
  ),
}));

const model: ModelPricing = {
  modelId: "deepseek-v4",
  displayName: "DeepSeek V4",
  inputCostPerMillion: "1",
  outputCostPerMillion: "3",
  cacheReadCostPerMillion: "0.0028",
  cacheCreationCostPerMillion: "0",
};

const PRICE_FIELDS = [
  { id: "inputCost", label: "输入成本" },
  { id: "outputCost", label: "输出成本" },
  { id: "cacheReadCost", label: "缓存读取成本" },
  { id: "cacheCreationCost", label: "缓存写入成本" },
] as const;

describe("PricingEditModal", () => {
  it("all price inputs have step=0.0001", () => {
    render(<PricingEditModal open model={model} onClose={() => {}} />);

    for (const { id } of PRICE_FIELDS) {
      const input = screen.getByLabelText(
        /每百万 tokens/ as unknown as string,
        {
          selector: `#${id}`,
        },
      ) as HTMLInputElement;
      expect(input).toHaveAttribute("step", "0.0001");
    }
  });

  it("accepts precise cache read cost like 0.0028", () => {
    render(<PricingEditModal open model={model} onClose={() => {}} />);

    const cacheReadInput = document.getElementById(
      "cacheReadCost",
    ) as HTMLInputElement;
    expect(cacheReadInput.value).toBe("0.0028");
    expect(cacheReadInput.checkValidity()).toBe(true);
  });

  it("allows user to input sub-cent prices via change event", () => {
    render(<PricingEditModal open model={model} onClose={() => {}} isNew />);

    const cacheReadInput = document.getElementById(
      "cacheReadCost",
    ) as HTMLInputElement;

    fireEvent.change(cacheReadInput, { target: { value: "0.0015" } });
    expect(cacheReadInput.value).toBe("0.0015");
  });

  it("normalises a new model ID before saving (FE-UI-30)", async () => {
    mutateAsync.mockReset();
    mutateAsync.mockResolvedValue(undefined);
    const blank = { ...model, modelId: "", displayName: "" };
    render(<PricingEditModal open model={blank} onClose={() => {}} isNew />);

    fireEvent.change(document.getElementById("modelId")!, {
      target: { value: "openai/GPT-4.1@2025" },
    });
    fireEvent.change(document.getElementById("displayName")!, {
      target: { value: "GPT 4.1" },
    });
    expect(screen.getByTestId("modelId-normalized")).toHaveTextContent(
      "Will be saved as gpt-4.1-2025",
    );
    fireEvent.submit(document.getElementById("pricing-form")!);

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutateAsync.mock.calls[0][0].modelId).toBe("gpt-4.1-2025");
  });

  it("shows an inline error for a duplicate model ID", () => {
    mutateAsync.mockReset();
    const blank = { ...model, modelId: "", displayName: "" };
    render(<PricingEditModal open model={blank} onClose={() => {}} isNew />);

    fireEvent.change(document.getElementById("modelId")!, {
      target: { value: "openai/gpt-4o" },
    });
    fireEvent.submit(document.getElementById("pricing-form")!);

    expect(screen.getByTestId("modelId-error")).toHaveTextContent(
      "Pricing for gpt-4o already exists.",
    );
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("shows a field-level error for an invalid price", () => {
    mutateAsync.mockReset();
    render(<PricingEditModal open model={model} onClose={() => {}} />);

    fireEvent.change(document.getElementById("outputCost")!, {
      target: { value: "-1" },
    });
    fireEvent.submit(document.getElementById("pricing-form")!);

    expect(screen.getByTestId("outputCost-error")).toBeInTheDocument();
    expect(screen.queryByTestId("inputCost-error")).toBeNull();
    expect(mutateAsync).not.toHaveBeenCalled();
  });
});
