import { useMemo } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderMeta, EndpointCandidate } from "@/types";
import { getProviderCodec } from "@/lib/providerCodecs";
import {
  findPresetEntry,
  presetSettingsConfig,
  type AnyProviderPreset,
} from "@/config/presetRegistry";

interface UseSpeedTestEndpointsProps {
  appId: AppId;
  selectedPresetId: string | null;
  /** The base URL currently shown in the form. */
  baseUrl: string;
  initialData?: {
    settingsConfig?: Record<string, unknown>;
    meta?: ProviderMeta;
  };
  /**
   * Custom endpoints the user added in an earlier visit to the speed-test
   * panel of this (unsaved) form. Fed back as custom candidates so reopening
   * the panel does not drop them (FE-PROV-45).
   */
  draftCustomEndpoints?: string[];
}

const sanitize = (url?: string) => (url ?? "").trim().replace(/\/+$/, "");

/**
 * Initial endpoint list of the speed-test panel.
 *
 * Sources, in order: draft custom endpoints, the current base URL, the stored
 * provider's endpoint (edit mode) and the selected preset's own endpoint plus
 * its `endpointCandidates`. Endpoint extraction goes through the app's
 * provider codec (FE-PROV-22). Saved custom endpoints of an existing provider
 * are loaded by EndpointSpeedTest itself.
 */
export function useSpeedTestEndpoints({
  appId,
  selectedPresetId,
  baseUrl,
  initialData,
  draftCustomEndpoints,
}: UseSpeedTestEndpointsProps): EndpointCandidate[] {
  return useMemo(() => {
    const codec = getProviderCodec(appId);
    const map = new Map<string, EndpointCandidate>();
    const add = (url: string | undefined, isCustom = false) => {
      const sanitized = sanitize(url);
      if (!sanitized || map.has(sanitized)) return;
      map.set(sanitized, { url: sanitized, isCustom });
    };

    draftCustomEndpoints?.forEach((url) => add(url, true));
    add(baseUrl);
    if (initialData?.settingsConfig) {
      add(codec.getEndpoint(initialData.settingsConfig));
    }

    const entry = findPresetEntry(appId, selectedPresetId);
    if (entry) {
      const preset = entry.preset as AnyProviderPreset & {
        endpointCandidates?: string[];
      };
      add(codec.getEndpoint(presetSettingsConfig(preset)));
      preset.endpointCandidates?.forEach((url) => add(url));
    }

    return Array.from(map.values());
  }, [appId, baseUrl, initialData, selectedPresetId, draftCustomEndpoints]);
}
