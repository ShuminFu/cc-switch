import { act, fireEvent, render, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import i18n from "i18next";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormProps,
} from "@/components/providers/forms/ProviderForm";
import {
  PresetSortMode,
  getPresetDisplayName,
  sortPresetEntries,
  type PresetEntry,
} from "@/components/providers/forms/ProviderPresetSelector";
import type { AppId } from "@/lib/api";
import { server } from "../../msw/server";
import { setSettings } from "../../msw/state";
import { createTestQueryClient } from "../../utils/testQueryClient";
import { toastErrorMock } from "./mocks";

/**
 * settingsConfig golden harness (CCS-189).
 *
 * For every preset of an app (plus "custom") it renders the real ProviderForm
 * in create mode, selects the preset, fills only what the form refuses to
 * submit without (provider key, API key, required template fields) with fixed
 * values, submits and snapshots the onSubmit payload. Managed OAuth reports
 * one signed-in account so Copilot/Codex presets submit too. It then re-opens
 * that payload in edit mode, saves it unchanged and expects the same payload
 * back.
 *
 * Each test file must mock `sonner` and `@/components/JsonEditor` with the
 * factories in `./mocks` (vi.mock is hoisted per file, so it cannot live here).
 */

export const GOLDEN_API_KEY = "sk-golden-test-key";
const SUBMIT_LABEL = "Save";
const SAVE_ANYWAY_LABEL = "仍要保存";
const TAURI_ENDPOINT = "http://tauri.local";

type SubmittedValues = Parameters<ProviderFormProps["onSubmit"]>[0];
type Payload = Record<string, unknown>;

export type GoldenOutcome = {
  /** Inputs the harness had to fill before the form would submit. */
  filled: string[];
} & (
  | { submitted: true; payload: Payload }
  | { submitted: false; errors: unknown[] }
);

const t = (key: string) => i18n.t(key);

/**
 * Answer the commands the form calls on mount that the shared MSW handlers
 * do not cover. Unanswered they go to the network, which is slow and lets the
 * response race the submit.
 */
const installGoldenHandlers = () => {
  server.use(
    http.post(`${TAURI_ENDPOINT}/get_common_config_snippet`, () =>
      HttpResponse.json(null),
    ),
    http.post(`${TAURI_ENDPOINT}/get_hermes_live_provider_ids`, () =>
      HttpResponse.json([]),
    ),
    // No backend default routes: proxy-mode presets keep their own routes.
    http.post(`${TAURI_ENDPOINT}/get_claude_desktop_default_routes`, () =>
      HttpResponse.json([]),
    ),
    http.post(`${TAURI_ENDPOINT}/auth_get_status`, async ({ request }) => {
      const { authProvider } = (await request.json()) as {
        authProvider: string;
      };
      // A signed-in managed account, so OAuth presets (Copilot, Codex)
      // submit and their settingsConfig is pinned too.
      return HttpResponse.json({
        provider: authProvider,
        authenticated: true,
        default_account_id: "golden-account",
        accounts: [
          {
            id: "golden-account",
            provider: authProvider,
            login: "golden-user",
            avatar_url: null,
            authenticated_at: 0,
            is_default: true,
            github_domain: "github.com",
          },
        ],
      });
    }),
  );
};

const renderForm = (
  appId: AppId,
  initialData?: ProviderFormProps["initialData"],
) => {
  // Skip the one-time common-config notice dialog; it covers the form.
  setSettings({ commonConfigConfirmed: true });
  const onSubmit = vi.fn<(values: SubmittedValues) => void>();
  const utils = render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId={appId}
        providerId={initialData ? "golden-existing" : undefined}
        submitLabel={SUBMIT_LABEL}
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={initialData}
      />
    </QueryClientProvider>,
  );
  return { ...utils, onSubmit };
};

/** Let pending queries/effects (settings, live ids, default routes) settle. */
const flush = async () => {
  for (let i = 0; i < 5; i += 1) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
};

// Text lookups instead of *ByRole: role queries take ~0.5s each on this form.
const buttonByText = (label: string) =>
  Array.from(document.querySelectorAll("button")).find(
    (button) => button.textContent?.trim() === label,
  );

/** Click a preset by its position in the grid (custom first, then sorted). */
const selectPreset = (entries: PresetEntry[], id: string) => {
  const customButton = buttonByText("providerPreset.custom");
  expect(customButton).toBeDefined();
  const index = sortPresetEntries(
    entries,
    PresetSortMode.Original,
    t,
  ).findIndex((entry) => entry.id === id);
  const button = customButton!.parentElement!.children[index + 1];
  const entry = entries.find((item) => item.id === id)!;
  expect(button?.textContent).toContain(getPresetDisplayName(entry.preset, t));
  fireEvent.click(button);
};

const fieldLabel = (input: HTMLInputElement) =>
  input.id || input.name || input.placeholder || "unnamed";

export const GOLDEN_PROVIDER_KEY = "golden-provider";

/** Provider key inputs (opencode/openclaw/hermes): required, but not `required`. */
const PROVIDER_KEY_SELECTOR = "#opencode-key, #openclaw-key, #hermes-key";

/**
 * Fill the API key, the provider key and any empty required input with fixed
 * values.
 */
const fillRequiredInputs = (container: HTMLElement): string[] => {
  const filled: string[] = [];
  const providerKey = container.querySelector<HTMLInputElement>(
    PROVIDER_KEY_SELECTOR,
  );
  if (providerKey && !providerKey.disabled && !providerKey.value) {
    fireEvent.change(providerKey, { target: { value: GOLDEN_PROVIDER_KEY } });
    filled.push(fieldLabel(providerKey));
  }
  const apiKey = container.querySelector<HTMLInputElement>(
    'input[type="password"]',
  );
  if (apiKey && !apiKey.disabled && !apiKey.value) {
    fireEvent.change(apiKey, { target: { value: GOLDEN_API_KEY } });
    filled.push(`${fieldLabel(apiKey)} (api key)`);
  }
  for (const input of Array.from(
    container.querySelectorAll<HTMLInputElement>("input[required]"),
  )) {
    if (input.disabled || input.value) continue;
    const label = fieldLabel(input);
    fireEvent.change(input, { target: { value: `golden-${label}` } });
    filled.push(label);
  }
  return filled;
};

/** Parse settingsConfig so the snapshot diff is per-field, not one string. */
const normalizePayload = (values: SubmittedValues): Payload => {
  const { settingsConfig, ...rest } = values;
  return JSON.parse(
    JSON.stringify({ ...rest, settingsConfig: JSON.parse(settingsConfig) }),
  );
};

const submitAndCollect = async (
  container: HTMLElement,
  onSubmit: ReturnType<typeof renderForm>["onSubmit"],
  filled: string[],
): Promise<GoldenOutcome> => {
  // jsdom applies native constraint validation: an invalid form never fires
  // submit, so report it instead of waiting for an onSubmit that won't come.
  const invalid = Array.from(container.querySelectorAll("input")).filter(
    (input) => !input.checkValidity(),
  );
  if (invalid.length > 0) {
    return {
      filled,
      submitted: false,
      errors: invalid.map((input) => `invalid input: ${fieldLabel(input)}`),
    };
  }

  toastErrorMock.mockClear();
  fireEvent.click(buttonByText(SUBMIT_LABEL)!);

  await waitFor(() => {
    // Soft issues (empty model list, ...) open a "save anyway" dialog.
    const confirm = buttonByText(SAVE_ANYWAY_LABEL);
    if (confirm) fireEvent.click(confirm);
    expect(
      onSubmit.mock.calls.length + toastErrorMock.mock.calls.length,
    ).toBeGreaterThan(0);
  });

  if (onSubmit.mock.calls.length > 0) {
    expect(onSubmit).toHaveBeenCalledTimes(1);
    return {
      filled,
      submitted: true,
      payload: normalizePayload(onSubmit.mock.calls[0][0]),
    };
  }
  return {
    filled,
    submitted: false,
    errors: toastErrorMock.mock.calls.map((call) => call[0]),
  };
};

/** Create mode: pick a preset (or "custom") and submit its defaults. */
const submitPresetDefaults = async (
  appId: AppId,
  entries: PresetEntry[],
  presetId: string,
): Promise<GoldenOutcome> => {
  const { container, onSubmit, unmount } = renderForm(appId);
  await flush();
  if (presetId === "custom") {
    fireEvent.click(buttonByText("providerPreset.custom")!);
  } else {
    selectPreset(entries, presetId);
  }
  await flush();
  const filled = fillRequiredInputs(container);
  await flush();
  const outcome = await submitAndCollect(container, onSubmit, filled);
  unmount();
  return outcome;
};

/** Edit mode: re-open a submitted payload and save it without changes. */
const submitEditUnchanged = async (
  appId: AppId,
  payload: Payload,
): Promise<GoldenOutcome> => {
  const { container, onSubmit, unmount } = renderForm(appId, {
    name: payload.name as string,
    websiteUrl: payload.websiteUrl as string | undefined,
    notes: payload.notes as string | undefined,
    settingsConfig: payload.settingsConfig as Record<string, unknown>,
    category: payload.presetCategory as never,
    meta: payload.meta as never,
    icon: payload.icon as string | undefined,
    iconColor: payload.iconColor as string | undefined,
  });
  await flush();
  const outcome = await submitAndCollect(container, onSubmit, []);
  unmount();
  return outcome;
};

/**
 * Preset bookkeeping that only a create-mode submit carries. `providerKey` is
 * the new provider's id (opencode/openclaw/hermes); edit keeps the existing id.
 * `suggestedDefaults` is the OpenClaw preset's default-model suggestion.
 */
const CREATE_ONLY_FIELDS = [
  "providerKey",
  "suggestedDefaults",
  "presetId",
  "presetCategory",
  "isPartner",
  "partnerPromotionKey",
];

const withoutCreateOnlyFields = (payload: Payload) => {
  const copy = { ...payload };
  for (const field of CREATE_ONLY_FIELDS) delete copy[field];
  return copy;
};

export interface GoldenSuiteOptions {
  /**
   * Presets whose edit-mode save does not reproduce the create payload today,
   * keyed by preset id with the reason. Their edit payload is snapshotted
   * instead; the test fails once the drift is gone so the entry gets removed.
   */
  knownRoundTripDrift?: Record<string, string>;
}

export const describeSettingsConfigGoldens = (
  appId: AppId,
  entries: PresetEntry[],
  { knownRoundTripDrift = {} }: GoldenSuiteOptions = {},
) => {
  const cases: Array<[string, string]> = [
    ["custom", "custom"],
    ...entries.map((entry): [string, string] => [entry.id, entry.preset.name]),
  ];

  describe(`ProviderForm settingsConfig goldens: ${appId}`, () => {
    beforeEach(() => {
      installGoldenHandlers();
      // Generated ids (OMO provider keys, row keys) must be stable in goldens.
      let seq = 0;
      vi.spyOn(crypto, "randomUUID").mockImplementation(() => {
        seq += 1;
        return `00000000-0000-4000-8000-${String(seq).padStart(12, "0")}`;
      });
    });

    it.each(cases)(
      "%s %s",
      async (presetId) => {
        const created = await submitPresetDefaults(appId, entries, presetId);
        expect(created).toMatchSnapshot("create");
        if (!created.submitted) return;

        const edited = await submitEditUnchanged(appId, created.payload);
        expect(edited.submitted).toBe(true);
        if (!edited.submitted) return;

        const before = withoutCreateOnlyFields(created.payload);
        const after = withoutCreateOnlyFields(edited.payload);
        if (presetId in knownRoundTripDrift) {
          expect(after).not.toEqual(before);
          expect(after).toMatchSnapshot("edit (known drift)");
        } else {
          expect(after).toEqual(before);
        }
        // Two full form renders per case; leave headroom on loaded CI runners.
      },
      30_000,
    );
  });
};
