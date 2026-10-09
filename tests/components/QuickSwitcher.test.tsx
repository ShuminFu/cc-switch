import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import {
  QuickSwitcher,
  sortProvidersForSwitcher,
} from "@/components/providers/QuickSwitcher";
import type { Provider } from "@/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      let text = (options?.defaultValue as string | undefined) ?? key;
      for (const [name, value] of Object.entries(options ?? {})) {
        text = text.split(`{{${name}}}`).join(String(value));
      }
      return text;
    },
  }),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children, ...props }: any) => (
    <div {...props}>{children}</div>
  ),
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: ({ name }: { name: string }) => <span>{name[0]}</span>,
}));

const providers: Record<string, Provider> = {
  b: { id: "b", name: "Beta Relay", settingsConfig: {}, sortIndex: 1 },
  a: { id: "a", name: "Alpha Official", settingsConfig: {}, sortIndex: 0 },
  c: { id: "c", name: "Charlie", settingsConfig: {} },
};

describe("sortProvidersForSwitcher", () => {
  it("orders by sortIndex then name, unsorted last", () => {
    expect(sortProvidersForSwitcher(providers).map((p) => p.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
  });
});

describe("QuickSwitcher", () => {
  beforeAll(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("renders nothing while closed", () => {
    render(
      <QuickSwitcher
        open={false}
        onOpenChange={() => {}}
        appLabel="Claude"
        providers={providers}
        currentProviderId="a"
        onSelect={() => {}}
      />,
    );
    expect(screen.queryByTestId("quick-switcher")).toBeNull();
  });

  it("lists providers with the current one marked and selects on click", () => {
    const onSelect = vi.fn();
    render(
      <QuickSwitcher
        open
        onOpenChange={() => {}}
        appLabel="Claude"
        providers={providers}
        currentProviderId="a"
        onSelect={onSelect}
      />,
    );
    expect(screen.getByTestId("quick-switch-a")).toHaveTextContent("当前");
    expect(screen.getByTestId("quick-switch-b")).not.toHaveTextContent("当前");

    fireEvent.click(screen.getByTestId("quick-switch-b"));
    expect(onSelect).toHaveBeenCalledWith(providers.b);
  });

  it("filters providers by the typed query", () => {
    render(
      <QuickSwitcher
        open
        onOpenChange={() => {}}
        appLabel="Claude"
        providers={providers}
        currentProviderId="a"
        onSelect={() => {}}
      />,
    );
    fireEvent.change(screen.getByPlaceholderText("搜索 Claude 供应商…"), {
      target: { value: "char" },
    });
    expect(screen.getByTestId("quick-switch-c")).toBeInTheDocument();
    expect(screen.queryByTestId("quick-switch-a")).toBeNull();
  });
});
