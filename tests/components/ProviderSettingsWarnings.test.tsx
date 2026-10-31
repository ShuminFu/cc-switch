import { render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ProviderSettingsWarnings } from "@/components/providers/forms/ProviderSettingsWarnings";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const renderWarnings = (appId: "codex" | "opencode", settingsConfig: string) =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderSettingsWarnings appId={appId} settingsConfig={settingsConfig} />
    </QueryClientProvider>,
  );

describe("ProviderSettingsWarnings (validate_provider_settings)", () => {
  it("renders backend issues inline with English defaults", async () => {
    const api = mockInvoke({
      validate_provider_settings: [
        {
          path: "model_providers.custom.wire_api",
          key: "validation.codex.wireApi.invalid",
          params: { value: "response", allowed: ["responses", "chat"] },
        },
        {
          path: "model_provider",
          key: "validation.codex.modelProvider.undefined",
          params: { value: "custom" },
        },
      ],
    });
    const settings = JSON.stringify({ config: 'model_provider = "custom"' });
    renderWarnings("codex", settings);

    expect(
      await screen.findByText(
        'wire_api "response" is not supported; use one of: responses, chat',
        {},
        { timeout: 2000 },
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        'model_provider "custom" has no [model_providers.custom] table',
      ),
    ).toBeInTheDocument();
    expect(api.callsTo("validate_provider_settings")[0]?.args).toEqual({
      app: "codex",
      settings: { config: 'model_provider = "custom"' },
    });
  });

  it("does not call the validator for apps it does not cover or invalid JSON", async () => {
    const api = mockInvoke({ validate_provider_settings: [] });
    renderWarnings("opencode", "{}");
    renderWarnings("codex", "{not json");
    await new Promise((resolve) => setTimeout(resolve, 600));
    await waitFor(() =>
      expect(api.callsTo("validate_provider_settings")).toHaveLength(0),
    );
    expect(screen.queryByTestId("provider-settings-warnings")).toBeNull();
  });
});
