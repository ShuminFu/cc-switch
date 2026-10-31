import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import EndpointSpeedTest from "@/components/providers/forms/EndpointSpeedTest";
import { applyCustomEndpoints } from "@/components/providers/forms/shell/applyCustomEndpoints";

const api = vi.hoisted(() => ({
  getCustomEndpoints: vi.fn(),
  addCustomEndpoint: vi.fn(),
  removeCustomEndpoint: vi.fn(),
  testApiEndpoints: vi.fn(),
}));

vi.mock("@/lib/api/endpoints", () => ({ endpointsApi: api }));
vi.mock("@/lib/api", () => ({ endpointsApi: api }));

beforeEach(() => {
  Object.values(api).forEach((fn) => fn.mockReset());
  api.getCustomEndpoints.mockResolvedValue([
    { url: "https://stored.example", addedAt: 1 },
  ]);
  api.addCustomEndpoint.mockResolvedValue(undefined);
  api.removeCustomEndpoint.mockResolvedValue(undefined);
});

function renderPanel(
  props: Partial<React.ComponentProps<typeof EndpointSpeedTest>> = {},
) {
  const onCustomEndpointsChange = vi.fn();
  const onClose = vi.fn();
  render(
    <EndpointSpeedTest
      appId="claude"
      providerId="p1"
      value="https://main.example"
      onChange={vi.fn()}
      initialEndpoints={[{ url: "https://main.example" }]}
      visible
      onClose={onClose}
      autoSelect={false}
      onAutoSelectChange={vi.fn()}
      onCustomEndpointsChange={onCustomEndpointsChange}
      {...props}
    />,
  );
  return { onCustomEndpointsChange, onClose };
}

describe("EndpointSpeedTest in edit mode (FE-PROV-10)", () => {
  it("hands the edited list to the form on Save instead of writing it", async () => {
    const { onCustomEndpointsChange, onClose } = renderPanel();
    await screen.findByText("https://stored.example");

    const removeStored = screen
      .getAllByRole("button", {
        name: /Remove endpoint|endpointTest\.removeEndpoint/,
      })
      .find((button) =>
        button.parentElement?.parentElement?.textContent?.includes(
          "stored.example",
        ),
      );
    expect(removeStored).toBeDefined();
    fireEvent.click(removeStored!);
    fireEvent.click(screen.getByRole("button", { name: /common\.save/ }));

    expect(onClose).toHaveBeenCalled();
    expect(api.addCustomEndpoint).not.toHaveBeenCalled();
    expect(api.removeCustomEndpoint).not.toHaveBeenCalled();
    expect(onCustomEndpointsChange).toHaveBeenCalledTimes(1);
    expect(onCustomEndpointsChange.mock.calls[0][0]).not.toContain(
      "https://stored.example",
    );
  });

  it("starts from pending endpoints rather than the stored list", async () => {
    renderPanel({ pendingCustomEndpoints: ["https://pending.example"] });
    await screen.findByText("https://pending.example");
    expect(api.getCustomEndpoints).not.toHaveBeenCalled();
    expect(screen.queryByText("https://stored.example")).toBeNull();
  });

  it("Cancel neither writes nor reports anything", async () => {
    const { onCustomEndpointsChange } = renderPanel();
    await screen.findByText("https://stored.example");
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(onCustomEndpointsChange).not.toHaveBeenCalled();
    expect(api.addCustomEndpoint).not.toHaveBeenCalled();
    expect(api.removeCustomEndpoint).not.toHaveBeenCalled();
  });
});

describe("applyCustomEndpoints", () => {
  it("writes only the difference to the stored list", async () => {
    const result = await applyCustomEndpoints("claude", "p1", [
      "https://new.example/",
    ]);
    expect(result).toEqual({
      added: ["https://new.example"],
      removed: ["https://stored.example"],
    });
    expect(api.removeCustomEndpoint).toHaveBeenCalledWith(
      "claude",
      "p1",
      "https://stored.example",
    );
    expect(api.addCustomEndpoint).toHaveBeenCalledWith(
      "claude",
      "p1",
      "https://new.example",
    );
  });

  it("does nothing when the list is unchanged", async () => {
    await waitFor(() =>
      applyCustomEndpoints("claude", "p1", ["https://stored.example"]),
    );
    expect(api.addCustomEndpoint).not.toHaveBeenCalled();
    expect(api.removeCustomEndpoint).not.toHaveBeenCalled();
  });
});
