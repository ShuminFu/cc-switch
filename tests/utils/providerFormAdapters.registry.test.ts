import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ALL_APP_IDS, getAppDefinition } from "@/config/apps";
import {
  PROVIDER_FORM_ADAPTERS,
  getProviderFormAdapter,
} from "@/components/providers/forms/adapters";

describe("provider form adapter registry (FE-PROV-17)", () => {
  it("resolves an adapter for every app through the app registry", () => {
    for (const appId of ALL_APP_IDS) {
      const adapterId = getAppDefinition(appId).providerForm;
      expect(PROVIDER_FORM_ADAPTERS[adapterId]).toBeDefined();
      const adapter = getProviderFormAdapter(appId);
      if (adapter.kind === "shell") {
        expect(() => JSON.parse(adapter.defaultSettingsConfig)).not.toThrow();
      }
    }
  });

  it("keeps the form shell small and free of per-app branches", () => {
    const source = fs.readFileSync(
      path.resolve(
        __dirname,
        "../../src/components/providers/forms/ProviderForm.tsx",
      ),
      "utf8",
    );
    expect(source.split("\n").length).toBeLessThan(400);
    expect(source).not.toMatch(/appId\s*[!=]==/);
  });
});
