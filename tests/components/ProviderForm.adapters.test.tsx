import type { ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormProps,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import type { AppId } from "@/lib/api";

/**
 * Characterisation tests for the per-app provider form adapters
 * (FE-PROV-17 / FE-PROV-32): render the real ProviderForm for every app,
 * submit, and pin the payload handed to `onSubmit`. These guard the
 * shell/adapter split against behaviour drift.
 */

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
    id,
  }: {
    value: string;
    onChange: (value: string) => void;
    id?: string;
  }) => (
    <textarea
      data-testid={id ? `json-editor-${id}` : "json-editor"}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

vi.setConfig({ testTimeout: 30000 });

function renderWithClient(node: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{node}</QueryClientProvider>,
  );
}

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
}

async function submitAndCapture(
  onSubmit: ReturnType<typeof vi.fn>,
): Promise<ProviderFormValues> {
  await settle();
  fireEvent.submit(document.getElementById("provider-form")!);
  const saveAnyway = await screen
    .findByRole("button", { name: "仍要保存" }, { timeout: 400 })
    .catch(() => null);
  if (saveAnyway) fireEvent.click(saveAnyway);
  await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
  return onSubmit.mock.calls[0][0] as ProviderFormValues;
}

// Payloads cross the IPC boundary as JSON, so keys holding `undefined` are
// irrelevant; normalise them away so the snapshots pin behaviour only.
function normalizePayload(values: ProviderFormValues) {
  return JSON.parse(
    JSON.stringify({
      ...values,
      settingsConfig: JSON.parse(values.settingsConfig),
    }),
  ) as Record<string, unknown>;
}

const EDIT_CASES: Array<{
  appId: AppId;
  providerId: string;
  initialData: NonNullable<ProviderFormProps["initialData"]>;
}> = [
  {
    appId: "claude",
    providerId: "claude-1",
    initialData: {
      name: "Claude Relay",
      websiteUrl: "https://relay.example.com",
      category: "third_party",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://relay.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-claude",
          ANTHROPIC_MODEL: "claude-x",
        },
      },
      meta: { apiFormat: "anthropic", costMultiplier: "1.5" },
    },
  },
  {
    appId: "codex",
    providerId: "codex-1",
    initialData: {
      name: "Codex Relay",
      category: "third_party",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-codex" },
        config:
          'model_provider = "relay"\nmodel = "gpt-5"\n\n[model_providers.relay]\nname = "relay"\nbase_url = "https://codex.example.com/v1"\nwire_api = "responses"\n',
      },
    },
  },
  {
    appId: "gemini",
    providerId: "gemini-1",
    initialData: {
      name: "Gemini Relay",
      category: "third_party",
      settingsConfig: {
        env: {
          GEMINI_API_KEY: "g-key",
          GOOGLE_GEMINI_BASE_URL: "https://gemini.example.com",
          GEMINI_MODEL: "gemini-x",
        },
        config: { general: { previewFeatures: true } },
      },
    },
  },
  {
    appId: "opencode",
    providerId: "my-opencode",
    initialData: {
      name: "OpenCode Relay",
      category: "third_party",
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        options: {
          baseURL: "https://opencode.example.com/v1",
          apiKey: "sk-oc",
        },
        models: { "model-a": { name: "Model A" } },
      },
    },
  },
  {
    appId: "opencode",
    providerId: "omo-abc",
    initialData: {
      name: "OMO",
      category: "omo",
      settingsConfig: {
        agents: { sisyphus: { model: "relay/model-a" } },
        otherFields: { foo: 1 },
      },
    },
  },
  {
    appId: "openclaw",
    providerId: "my-openclaw",
    initialData: {
      name: "OpenClaw Relay",
      category: "third_party",
      settingsConfig: {
        baseUrl: "https://openclaw.example.com/v1",
        apiKey: "sk-ocl",
        api: "openai-completions",
        models: [{ id: "m1", name: "M1" }],
      },
    },
  },
  {
    appId: "hermes",
    providerId: "my-hermes",
    initialData: {
      name: "Hermes Relay",
      category: "third_party",
      settingsConfig: {
        name: "my-hermes",
        base_url: "https://hermes.example.com/v1",
        api_key: "sk-h",
        models: [{ id: "h1" }],
      },
    },
  },
];

describe("ProviderForm per-app submit payloads (edit mode)", () => {
  for (const testCase of EDIT_CASES) {
    it(`${testCase.appId}/${testCase.initialData.category} submits the unchanged provider`, async () => {
      const onSubmit = vi.fn();
      renderWithClient(
        <ProviderForm
          appId={testCase.appId}
          providerId={testCase.providerId}
          submitLabel="save"
          onSubmit={onSubmit}
          onCancel={() => {}}
          initialData={testCase.initialData}
        />,
      );
      await screen.findByDisplayValue(testCase.initialData.name!);
      const payload = await submitAndCapture(onSubmit);
      expect(normalizePayload(payload)).toMatchSnapshot();
    });
  }
});

const ADD_CASES: Array<{ appId: AppId; presetName: string; key?: string }> = [
  { appId: "claude", presetName: "Shengsuanyun" },
  { appId: "codex", presetName: "Shengsuanyun" },
  { appId: "gemini", presetName: "Shengsuanyun" },
  { appId: "opencode", presetName: "Shengsuanyun", key: "ssy" },
  { appId: "openclaw", presetName: "Shengsuanyun", key: "ssy" },
  { appId: "hermes", presetName: "Shengsuanyun", key: "ssy" },
];

describe("ProviderForm preset application (add mode)", () => {
  for (const testCase of ADD_CASES) {
    it(`${testCase.appId} applies the ${testCase.presetName} preset`, async () => {
      const onSubmit = vi.fn();
      renderWithClient(
        <ProviderForm
          appId={testCase.appId}
          submitLabel="add"
          onSubmit={onSubmit}
          onCancel={() => {}}
        />,
      );
      fireEvent.click(
        await screen.findByRole("button", {
          name: new RegExp(testCase.presetName),
        }),
      );
      await waitFor(() =>
        expect(
          (document.querySelector('input[name="name"]') as HTMLInputElement)
            .value,
        ).not.toBe(""),
      );
      if (testCase.key) {
        const keyInput = document.getElementById(
          `${testCase.appId}-key`,
        ) as HTMLInputElement;
        fireEvent.change(keyInput, { target: { value: testCase.key } });
      }
      const payload = await submitAndCapture(onSubmit);
      const normalized = normalizePayload(payload);
      // The preset id is an implementation detail checked separately.
      expect(typeof normalized.presetId).toBe("string");
      expect({ ...normalized, presetId: "<preset>" }).toMatchSnapshot();
    });
  }
});
