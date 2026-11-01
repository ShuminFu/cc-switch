import { render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { UniversalProviderPanel } from "@/components/universal/UniversalProviderPanel";
import { queryKeys } from "@/lib/query/keys";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

describe("UniversalProviderPanel (FE-PROV-37)", () => {
  it("loads universal providers through the shared query cache", async () => {
    const api = mockInvoke({
      get_universal_providers: {
        u1: {
          id: "u1",
          name: "Shared Relay",
          providerType: "custom",
          apps: { claude: true, codex: true, gemini: false },
          baseUrl: "https://relay.example.com",
          apiKey: "sk-test",
          models: {},
        },
      },
    });
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <UniversalProviderPanel />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("Shared Relay")).toBeInTheDocument();
    expect(api.callsTo("get_universal_providers")).toHaveLength(1);
    expect(
      client.getQueryData(queryKeys.universalProviders.all),
    ).toHaveProperty("u1");

    // Invalidating the key (what save/delete do) refetches the list.
    await client.invalidateQueries({
      queryKey: queryKeys.universalProviders.all,
    });
    await waitFor(() =>
      expect(api.callsTo("get_universal_providers")).toHaveLength(2),
    );
  });
});
