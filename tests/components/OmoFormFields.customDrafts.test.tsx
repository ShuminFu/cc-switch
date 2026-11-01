import type { ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  OmoFormFields,
  reconcileCustomModels,
  type CustomModelItem,
} from "@/components/providers/forms/OmoFormFields";

const BUILTINS = new Set(["sisyphus"]);

describe("reconcileCustomModels (FE-PROV-06)", () => {
  it("keeps an empty draft row when an unrelated store entry changes", () => {
    const prev: CustomModelItem[] = [{ key: "", model: "", sourceKey: "" }];
    const next = reconcileCustomModels(
      prev,
      { sisyphus: { model: "new-builtin-model" } },
      BUILTINS,
    );
    expect(next).toEqual(prev);
  });

  it("keeps a keyed row that has no model yet", () => {
    const prev: CustomModelItem[] = [
      { key: "my-agent", model: "", sourceKey: "" },
    ];
    expect(reconcileCustomModels(prev, {}, BUILTINS)).toEqual(prev);
  });

  it("refreshes persisted rows from the store and appends new store rows", () => {
    const prev: CustomModelItem[] = [
      { key: "renamed", model: "m1", sourceKey: "old" },
      { key: "", model: "", sourceKey: "" },
    ];
    const next = reconcileCustomModels(
      prev,
      {
        sisyphus: { model: "x" },
        renamed: { model: "m1" },
        imported: { model: "m2" },
      },
      BUILTINS,
    );
    expect(next).toEqual([
      { key: "renamed", model: "m1", sourceKey: "renamed" },
      { key: "", model: "", sourceKey: "" },
      { key: "imported", model: "m2", sourceKey: "imported" },
    ]);
  });

  it("drops complete rows that were removed from the store", () => {
    const prev: CustomModelItem[] = [
      { key: "gone", model: "m1", sourceKey: "gone" },
    ];
    expect(reconcileCustomModels(prev, {}, BUILTINS)).toEqual([]);
  });
});

function withClient(node: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return <QueryClientProvider client={client}>{node}</QueryClientProvider>;
}

describe("OmoFormFields custom rows (FE-PROV-06)", () => {
  it("keeps a new custom agent row after a builtin agent's model changes", () => {
    const onAgentsChange = vi.fn();
    const props = {
      modelOptions: [{ value: "p/model-a", label: "model-a" }],
      onAgentsChange,
      categories: {},
      onCategoriesChange: vi.fn(),
      otherFieldsStr: "",
      onOtherFieldsStrChange: vi.fn(),
    };

    const { rerender } = render(
      withClient(<OmoFormFields {...props} agents={{}} />),
    );

    const before = screen.queryAllByPlaceholderText("agent key").length;
    fireEvent.click(screen.getAllByText("Custom")[0].closest("button")!);
    expect(screen.getAllByPlaceholderText("agent key")).toHaveLength(
      before + 1,
    );

    // Simulate the parent store update caused by picking a builtin model.
    rerender(
      withClient(
        <OmoFormFields
          {...props}
          agents={{ sisyphus: { model: "p/model-a" } }}
        />,
      ),
    );

    expect(screen.getAllByPlaceholderText("agent key")).toHaveLength(
      before + 1,
    );
  });
});
