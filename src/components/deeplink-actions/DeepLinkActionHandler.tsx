import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import type { ProfileScope } from "@/lib/api/profiles";
import {
  useApplyProfileMutation,
  useProfilesQuery,
} from "@/lib/query/profiles";
import { APP_REGISTRY, isAppId } from "@/config/apps";
import { useNavigation, type ViewId } from "@/app/navigation";
import { useProviderActions } from "@/hooks/useProviderActions";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { extractErrorMessage } from "@/utils/errorUtils";

/** Mirrors `features/deeplink_nav.rs` (PRODUCT-20). */
export type DeepLinkAction =
  | { kind: "open"; view: string; tab?: string; app?: string }
  | { kind: "switch"; app: string; provider: string }
  | { kind: "applyProfile"; name: string; scope?: string };

export const takeDeepLinkAction = () =>
  invoke<DeepLinkAction | null>("take_deeplink_action");

/** Resolve a provider by id, then by unique case-insensitive name. */
export function resolveProvider(
  providers: Record<string, Provider>,
  ref: string,
): Provider | undefined {
  if (providers[ref]) return providers[ref];
  const needle = ref.trim().toLowerCase();
  const matches = Object.values(providers).filter(
    (p) => p.name.trim().toLowerCase() === needle,
  );
  return matches.length === 1 ? matches[0] : undefined;
}

const VIEW_TARGET: Record<string, { view: ViewId; tab?: string }> = {
  providers: { view: "providers" },
  settings: { view: "settings" },
  usage: { view: "settings", tab: "usage" },
  proxy: { view: "settings", tab: "proxy" },
  mcp: { view: "mcp" },
  skills: { view: "skills" },
  prompts: { view: "prompts" },
  sessions: { view: "sessions" },
  universal: { view: "universal" },
};

type PendingSwitch = { app: AppId; provider: Provider };
type PendingProfile = { id: string; name: string; scope: ProfileScope };

/**
 * Executes `ccswitch://v1/open|switch|profile/apply` links. Navigation runs
 * immediately; switching a provider or applying a project asks first.
 */
export function DeepLinkActionHandler() {
  const { t } = useTranslation();
  const { navigate, setActiveApp, activeApp } = useNavigation();
  const { data: profilesData } = useProfilesQuery();
  const [pendingSwitch, setPendingSwitch] = useState<PendingSwitch | null>(
    null,
  );
  const [pendingProfile, setPendingProfile] = useState<PendingProfile | null>(
    null,
  );

  const fail = useCallback(
    (message: string) =>
      toast.error(
        t("deeplinkAction.failed", {
          defaultValue: "Deep link could not be handled: {{error}}",
          error: message,
        }),
      ),
    [t],
  );

  const handle = useCallback(
    async (action: DeepLinkAction) => {
      if (action.kind === "open") {
        const target = VIEW_TARGET[action.view];
        if (!target) return fail(action.view);
        if (action.app && isAppId(action.app)) setActiveApp(action.app);
        const tab = action.tab ?? target.tab;
        if (target.view === "settings") {
          navigate("settings", tab ? { tab } : undefined);
        } else {
          navigate(target.view);
        }
        return;
      }
      if (action.kind === "switch") {
        if (!isAppId(action.app)) return fail(action.app);
        try {
          const providers = await providersApi.getAll(action.app);
          const provider = resolveProvider(providers, action.provider);
          if (!provider) {
            return fail(
              t("deeplinkAction.providerNotFound", {
                defaultValue: "No provider '{{provider}}' for {{app}}",
                provider: action.provider,
                app: APP_REGISTRY[action.app].label,
              }),
            );
          }
          setActiveApp(action.app);
          navigate("providers");
          setPendingSwitch({ app: action.app, provider });
        } catch (error) {
          fail(extractErrorMessage(error) || String(error));
        }
        return;
      }
      const profiles = profilesData?.profiles ?? [];
      const needle = action.name.trim().toLowerCase();
      const profile = profiles.find(
        (p) => p.id === action.name || p.name.trim().toLowerCase() === needle,
      );
      if (!profile) {
        return fail(
          t("deeplinkAction.profileNotFound", {
            defaultValue: "No project named '{{name}}'",
            name: action.name,
          }),
        );
      }
      const scope = (
        action.scope && isAppId(action.scope) ? action.scope : activeApp
      ) as ProfileScope;
      setPendingProfile({ id: profile.id, name: profile.name, scope });
    },
    [activeApp, fail, navigate, profilesData, setActiveApp, t],
  );

  useEffect(() => {
    let disposed = false;
    const drain = async () => {
      try {
        const action = await takeDeepLinkAction();
        if (action && !disposed) await handle(action);
      } catch (error) {
        console.error("[DeepLinkAction] take failed", error);
      }
    };
    void drain();
    let unlisten: (() => void) | undefined;
    void listen("deeplink-action", () => void drain()).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [handle]);

  return (
    <>
      {pendingSwitch && (
        <SwitchConfirm
          key={`${pendingSwitch.app}:${pendingSwitch.provider.id}`}
          pending={pendingSwitch}
          onDone={() => setPendingSwitch(null)}
        />
      )}
      {pendingProfile && (
        <ProfileConfirm
          pending={pendingProfile}
          onDone={() => setPendingProfile(null)}
        />
      )}
    </>
  );
}

function SwitchConfirm({
  pending,
  onDone,
}: {
  pending: PendingSwitch;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const { switchProvider } = useProviderActions(pending.app);
  const [busy, setBusy] = useState(false);
  return (
    <ConfirmDialog
      isOpen
      variant="info"
      isPending={busy}
      title={t("deeplinkAction.switchTitle", {
        defaultValue: "Switch provider?",
      })}
      message={t("deeplinkAction.switchMessage", {
        defaultValue: "A link asks to switch {{app}} to “{{provider}}”.",
        app: APP_REGISTRY[pending.app].label,
        provider: pending.provider.name,
      })}
      confirmText={t("deeplinkAction.switchConfirm", {
        defaultValue: "Switch",
      })}
      onConfirm={() => {
        setBusy(true);
        void Promise.resolve(switchProvider(pending.provider))
          .catch(() => undefined)
          .finally(onDone);
      }}
      onCancel={onDone}
    />
  );
}

function ProfileConfirm({
  pending,
  onDone,
}: {
  pending: PendingProfile;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const apply = useApplyProfileMutation();
  return (
    <ConfirmDialog
      isOpen
      variant="info"
      isPending={apply.isPending}
      title={t("deeplinkAction.profileTitle", {
        defaultValue: "Apply project?",
      })}
      message={t("deeplinkAction.profileMessage", {
        defaultValue: "A link asks to apply the project “{{name}}”.",
        name: pending.name,
      })}
      confirmText={t("deeplinkAction.profileConfirm", {
        defaultValue: "Apply",
      })}
      onConfirm={() =>
        apply.mutate(
          { id: pending.id, scope: pending.scope },
          { onSettled: onDone },
        )
      }
      onCancel={onDone}
    />
  );
}
