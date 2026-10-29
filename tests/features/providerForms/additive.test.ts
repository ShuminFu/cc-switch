import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import {
  ADDITIVE_APP_META,
  additiveExistingKeys,
  additiveManagedAuth,
  additiveProviderKeyError,
  additiveProviderMeta,
  managedAuthLoginError,
  providerKeyLockState,
} from "@/features/providers/forms/additiveForm";
import { coreFormFromProvider } from "@/features/providers/forms/coreForm";

const t = ((key: string) => `t:${key}`) as unknown as TFunction;

const keyInput = {
  key: "relay",
  i18nPrefix: "openclaw" as const,
  isLocked: false,
  isLockStateLoading: false,
  existingKeys: [] as string[],
};

describe("additive app form helpers (CCS-192)", () => {
  it("merges existing keys without duplicates or the provider itself", () => {
    expect(
      additiveExistingKeys("self", ["a", "self", "b"], ["b", "c", "self"]),
    ).toEqual(["a", "b", "c"]);
    expect(additiveExistingKeys(undefined, [], [])).toEqual([]);
  });

  it("locks the key only in edit mode once it is in the live config", () => {
    const live = { liveProviderIds: ["relay"], isLiveProviderIdsLoading: true };
    expect(
      providerKeyLockState({ ...live, isEditMode: true, providerId: "relay" }),
    ).toEqual({ isLocked: true, isLockStateLoading: true });
    expect(
      providerKeyLockState({ ...live, isEditMode: true, providerId: "other" }),
    ).toEqual({ isLocked: false, isLockStateLoading: true });
    expect(
      providerKeyLockState({ ...live, isEditMode: false, providerId: "relay" }),
    ).toEqual({ isLocked: false, isLockStateLoading: false });
  });

  it("checks the provider key in order: required, format, loading, duplicate", () => {
    expect(additiveProviderKeyError({ ...keyInput, key: " " }, t)).toBe(
      "t:openclaw.providerKeyRequired",
    );
    expect(additiveProviderKeyError({ ...keyInput, key: "Bad_Key" }, t)).toBe(
      "t:openclaw.providerKeyInvalid",
    );
    expect(
      additiveProviderKeyError(
        { ...keyInput, isLockStateLoading: true, existingKeys: ["relay"] },
        t,
      ),
    ).toBe("t:providerForm.providerKeyStatusLoading");
    expect(
      additiveProviderKeyError({ ...keyInput, existingKeys: ["relay"] }, t),
    ).toBe("t:openclaw.providerKeyDuplicate");
    // 已锁定（就是它自己的 live key）时不算重复
    expect(
      additiveProviderKeyError(
        { ...keyInput, isLocked: true, existingKeys: ["relay"] },
        t,
      ),
    ).toBeNull();
    expect(
      additiveProviderKeyError(
        { ...keyInput, i18nPrefix: "hermes.form", key: "" },
        t,
      ),
    ).toBe("t:hermes.form.providerKeyRequired");
  });

  it("requires a signed-in account for imported managed providers", () => {
    const copilot = additiveManagedAuth(
      { meta: { providerType: "github_copilot" } },
      coreFormFromProvider(undefined),
    );
    const codex = additiveManagedAuth(
      { meta: { providerType: "codex_oauth" } },
      coreFormFromProvider(undefined),
    );
    const signedOut = {
      isCopilotAuthenticated: false,
      isCodexOauthAuthenticated: false,
    };
    expect(managedAuthLoginError(copilot, signedOut, t)).toBe(
      "t:copilot.loginRequired",
    );
    expect(managedAuthLoginError(codex, signedOut, t)).toBe(
      "t:codexOauth.loginRequired",
    );
    expect(
      managedAuthLoginError(
        codex,
        { ...signedOut, isCodexOauthAuthenticated: true },
        t,
      ),
    ).toBeNull();
    expect(
      managedAuthLoginError(
        additiveManagedAuth(undefined, coreFormFromProvider(undefined)),
        signedOut,
        t,
      ),
    ).toBeNull();
  });

  it("keeps the saved meta and clears the fields these apps do not edit", () => {
    const initialData = {
      meta: {
        endpointAutoSelect: false,
        costMultiplier: "1.5",
        apiFormat: "openai_chat" as const,
        customUserAgent: "ua",
        notesFlag: "kept",
      },
    };
    const meta = additiveProviderMeta(
      initialData as never,
      coreFormFromProvider(initialData as never),
    );
    expect(meta).toMatchObject({
      endpointAutoSelect: false,
      costMultiplier: "1.5",
      notesFlag: "kept",
    });
    for (const key of Object.keys(ADDITIVE_APP_META)) {
      expect(meta).toHaveProperty(key, undefined);
    }
    expect(meta).not.toHaveProperty("codexFastMode");
  });
});
