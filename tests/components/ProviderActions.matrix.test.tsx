import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { getProviderCodec } from "@/lib/providerCodecs";

type Props = Parameters<typeof ProviderActions>[0];

function renderActions(overrides: Partial<Props> = {}) {
  const props: Props = {
    appId: "claude",
    isCurrent: false,
    onSwitch: vi.fn(),
    onEdit: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    onTest: vi.fn(),
    onConfigureUsage: vi.fn(),
    ...overrides,
  };
  render(<ProviderActions {...props} />);
  return props;
}

/** The main (first) button of the action bar. */
const mainButton = () => screen.getAllByRole("button")[0];

// FE-PROV-53: behaviour of the per-app button-state matrix.
describe("ProviderActions state matrix", () => {
  it("switch mode: enable when not current, disabled 'in use' when current", () => {
    const props = renderActions();
    expect(mainButton()).toHaveTextContent("Enable");
    fireEvent.click(mainButton());
    expect(props.onSwitch).toHaveBeenCalledTimes(1);
  });

  it("current provider: main button disabled and delete unavailable", () => {
    const props = renderActions({ isCurrent: true });
    expect(mainButton()).toBeDisabled();
    expect(mainButton()).toHaveTextContent("In Use");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(props.onDelete).not.toHaveBeenCalled();
  });

  it("additive app in config: main button removes from config", () => {
    const onRemoveFromConfig = vi.fn();
    renderActions({
      appId: "opencode",
      isInConfig: true,
      onRemoveFromConfig,
    });
    expect(mainButton()).toHaveTextContent("Remove");
    fireEvent.click(mainButton());
    expect(onRemoveFromConfig).toHaveBeenCalledTimes(1);
  });

  it("additive app not in config: main button adds", () => {
    const props = renderActions({ appId: "openclaw", isInConfig: false });
    expect(mainButton()).toHaveTextContent("Add");
    fireEvent.click(mainButton());
    expect(props.onSwitch).toHaveBeenCalledTimes(1);
  });

  it("OpenClaw default model cannot be removed from config", () => {
    renderActions({
      appId: "openclaw",
      isInConfig: true,
      isDefaultModel: true,
      onRemoveFromConfig: vi.fn(),
    });
    expect(mainButton()).toBeDisabled();
  });

  it("failover mode toggles queue membership, never while a mutation is pending", () => {
    const onToggleFailover = vi.fn();
    const { rerender } = render(
      <ProviderActions
        appId="claude"
        isCurrent={false}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
        isAutoFailoverEnabled
        isInFailoverQueue={false}
        onToggleFailover={onToggleFailover}
      />,
    );
    fireEvent.click(mainButton());
    expect(onToggleFailover).toHaveBeenCalledWith(true);

    rerender(
      <ProviderActions
        appId="claude"
        isCurrent={false}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
        isAutoFailoverEnabled
        isInFailoverQueue
        isActionPending
        onToggleFailover={onToggleFailover}
      />,
    );
    fireEvent.click(mainButton());
    expect(onToggleFailover).toHaveBeenCalledTimes(1);
  });

  it("official provider blocked by the proxy cannot be enabled", () => {
    const props = renderActions({ isOfficialBlockedByProxy: true });
    expect(mainButton()).toBeDisabled();
    fireEvent.click(mainButton());
    expect(props.onSwitch).not.toHaveBeenCalled();
  });

  it("read-only providers keep Duplicate disabled", () => {
    const props = renderActions({ appId: "hermes", isReadOnly: true });
    const duplicate = screen
      .getAllByRole("button")
      .find((button) => button.querySelector(".lucide-copy"));
    expect(duplicate).toBeDisabled();
    fireEvent.click(duplicate!);
    expect(props.onDuplicate).not.toHaveBeenCalled();
  });
});

describe("official-provider heuristic", () => {
  it("treats a Claude config without a base URL as official, Bedrock-style relays as not", () => {
    const codec = getProviderCodec("claude");
    expect(codec.isOfficial({ env: {} })).toBe(true);
    expect(
      codec.isOfficial({
        env: { ANTHROPIC_BASE_URL: "https://bedrock-runtime.example.com" },
      }),
    ).toBe(false);
    expect(codec.isOfficial({ apiKey: "aws-key", env: {} })).toBe(true);
  });
});
