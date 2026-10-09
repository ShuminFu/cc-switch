import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SessionUsageSummary } from "@/components/sessions/SessionUsageSummary";
import { SessionItem } from "@/components/sessions/SessionItem";
import type { SessionUsageStat } from "@/types/usage";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      let text = (options?.defaultValue as string | undefined) ?? key;
      for (const [name, value] of Object.entries(options ?? {})) {
        text = text.split(`{{${name}}}`).join(String(value));
      }
      return text;
    },
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: any) => <div>{children}</div>,
  TooltipTrigger: ({ children }: any) => <div>{children}</div>,
  TooltipContent: ({ children }: any) => <div>{children}</div>,
}));

const usage: SessionUsageStat = {
  sessionId: "sess-1",
  requests: 3,
  totalCostUsd: "0.004200",
  inputTokens: 1200,
  outputTokens: 300,
  cacheReadTokens: 10000,
  cacheCreationTokens: 500,
  firstSeenAt: 1,
  lastSeenAt: 2,
};

describe("SessionUsageSummary", () => {
  it("renders nothing without attributed usage", () => {
    const { container } = render(<SessionUsageSummary />);
    expect(container).toBeEmptyDOMElement();
  });

  it("offers a jump to the request log only when a handler is given", () => {
    const onViewRequests = vi.fn();
    const { rerender } = render(<SessionUsageSummary usage={usage} />);
    expect(screen.queryByTestId("session-view-requests")).toBeNull();

    rerender(
      <SessionUsageSummary usage={usage} onViewRequests={onViewRequests} />,
    );
    fireEvent.click(screen.getByTestId("session-view-requests"));
    expect(onViewRequests).toHaveBeenCalledTimes(1);
  });

  it("shows cost, request count and total tokens", () => {
    render(<SessionUsageSummary usage={usage} />);
    expect(screen.getByTestId("session-usage-summary")).toHaveTextContent(
      "$0.0042 · 3 次请求 · 12.0K tokens",
    );
  });
});

describe("SessionItem cost badge", () => {
  const session = {
    providerId: "claude",
    sessionId: "sess-1",
    title: "Alpha",
    lastActiveAt: 1_700_000_000,
  };

  it("shows the session cost when usage is attributed", () => {
    render(
      <SessionItem
        session={session}
        isSelected={false}
        selectionMode={false}
        isChecked={false}
        usage={usage}
        onSelect={() => {}}
        onToggleChecked={() => {}}
      />,
    );
    expect(screen.getByTestId("session-item-cost")).toHaveTextContent(
      "$0.0042",
    );
  });

  it("omits the badge when no usage is attributed", () => {
    render(
      <SessionItem
        session={session}
        isSelected={false}
        selectionMode={false}
        isChecked={false}
        onSelect={() => {}}
        onToggleChecked={() => {}}
      />,
    );
    expect(screen.queryByTestId("session-item-cost")).not.toBeInTheDocument();
  });
});
