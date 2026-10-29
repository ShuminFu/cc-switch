import { useState, useCallback, useRef, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { authApi, settingsApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import type {
  ManagedAuthProvider,
  ManagedAuthStatus,
  ManagedAuthDeviceCodeResponse,
} from "@/lib/api";

type PollingState = "idle" | "polling" | "success" | "error";

export function useManagedAuth(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const queryKey = ["managed-auth-status", authProvider];

  const [pollingState, setPollingState] = useState<PollingState>("idle");
  const [deviceCode, setDeviceCode] =
    useState<ManagedAuthDeviceCodeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Each login attempt owns its own timer handles. Bumping attemptRef
  // invalidates every async continuation (onSuccess awaits, in-flight polls)
  // that belongs to an older attempt, so it can never touch newer state.
  const attemptRef = useRef(0);
  const mountedRef = useRef(false);
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const expiryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const {
    data: authStatus,
    isLoading: isLoadingStatus,
    refetch: refetchStatus,
  } = useQuery<ManagedAuthStatus>({
    queryKey,
    queryFn: () => authApi.authGetStatus(authProvider),
    staleTime: 30000,
  });

  const stopPolling = useCallback(() => {
    attemptRef.current += 1;
    if (pollTimerRef.current) {
      clearTimeout(pollTimerRef.current);
      pollTimerRef.current = null;
    }
    if (expiryTimerRef.current) {
      clearTimeout(expiryTimerRef.current);
      expiryTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      stopPolling();
    };
  }, [stopPolling]);

  const startLoginMutation = useMutation({
    mutationFn: (_attempt: number) =>
      authApi.authStartLogin(authProvider, githubDomain),
    onSuccess: async (response, attempt) => {
      const isCurrent = () =>
        mountedRef.current && attemptRef.current === attempt;
      if (!isCurrent()) return;

      // Clear any handles left over before arming this attempt's timers.
      stopPolling();
      const token = attemptRef.current;
      const isActive = () => mountedRef.current && attemptRef.current === token;

      setDeviceCode(response);
      setPollingState("polling");
      setError(null);

      try {
        await copyText(response.user_code);
      } catch (e) {
        console.debug("[ManagedAuth] Failed to copy user code:", e);
      }
      if (!isActive()) return;

      try {
        await settingsApi.openExternal(response.verification_uri);
      } catch (e) {
        console.debug("[ManagedAuth] Failed to open browser:", e);
      }
      if (!isActive()) return;

      // Add a small buffer on top of GitHub's suggested interval to avoid
      // hitting slow_down responses too aggressively during device polling.
      const interval = Math.max((response.interval || 5) + 3, 8) * 1000;
      const expiresAt = Date.now() + response.expires_in * 1000;

      const expire = () => {
        stopPolling();
        setPollingState("error");
        setError(t("managedAuth.deviceCodeExpired"));
      };

      // Chained setTimeout: the next poll is only scheduled once the current
      // one settles, so polls never overlap.
      const pollOnce = async () => {
        pollTimerRef.current = null;
        if (!isActive()) return;
        if (Date.now() > expiresAt) {
          expire();
          return;
        }

        try {
          const newAccount = await authApi.authPollForAccount(
            authProvider,
            response.device_code,
            githubDomain,
          );
          if (!isActive()) return;
          if (newAccount) {
            stopPolling();
            const doneToken = attemptRef.current;
            setPollingState("success");
            await refetchStatus();
            await queryClient.invalidateQueries({ queryKey });
            if (!mountedRef.current || attemptRef.current !== doneToken) return;
            setPollingState("idle");
            setDeviceCode(null);
            return;
          }
        } catch (e) {
          if (!isActive()) return;
          const errorMessage = e instanceof Error ? e.message : String(e);
          if (
            !errorMessage.includes("pending") &&
            !errorMessage.includes("slow_down")
          ) {
            stopPolling();
            setPollingState("error");
            setError(errorMessage);
            return;
          }
        }

        if (!isActive()) return;
        pollTimerRef.current = setTimeout(() => void pollOnce(), interval);
      };

      expiryTimerRef.current = setTimeout(expire, response.expires_in * 1000);
      void pollOnce();
    },
    onError: (e, attempt) => {
      if (!mountedRef.current || attemptRef.current !== attempt) return;
      setPollingState("error");
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const logoutMutation = useMutation({
    mutationFn: () => authApi.authLogout(authProvider),
    onSuccess: async () => {
      setPollingState("idle");
      setDeviceCode(null);
      setError(null);
      queryClient.setQueryData(queryKey, {
        provider: authProvider,
        authenticated: false,
        default_account_id: null,
        accounts: [],
      });
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: async (e) => {
      console.error("[ManagedAuth] Failed to logout:", e);
      setError(e instanceof Error ? e.message : String(e));
      await refetchStatus();
    },
  });

  const removeAccountMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authRemoveAccount(authProvider, accountId),
    onSuccess: async () => {
      setPollingState("idle");
      setDeviceCode(null);
      setError(null);
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (e) => {
      console.error("[ManagedAuth] Failed to remove account:", e);
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const setDefaultAccountMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authSetDefaultAccount(authProvider, accountId),
    onSuccess: async () => {
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (e) => {
      console.error("[ManagedAuth] Failed to set default account:", e);
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const startAuth = useCallback(() => {
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
    stopPolling();
    startLoginMutation.mutate(attemptRef.current);
  }, [startLoginMutation, stopPolling]);

  const cancelAuth = useCallback(() => {
    stopPolling();
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
  }, [stopPolling]);

  const logout = useCallback(() => {
    logoutMutation.mutate();
  }, [logoutMutation]);

  const removeAccount = useCallback(
    (accountId: string) => {
      removeAccountMutation.mutate(accountId);
    },
    [removeAccountMutation],
  );

  const setDefaultAccount = useCallback(
    (accountId: string) => {
      setDefaultAccountMutation.mutate(accountId);
    },
    [setDefaultAccountMutation],
  );

  const accounts = authStatus?.accounts ?? [];

  return {
    authStatus,
    isLoadingStatus,
    accounts,
    hasAnyAccount: accounts.length > 0,
    isAuthenticated: authStatus?.authenticated ?? false,
    defaultAccountId: authStatus?.default_account_id ?? null,
    migrationError: authStatus?.migration_error ?? null,
    pollingState,
    deviceCode,
    error,
    isPolling: pollingState === "polling",
    isStartingAuth: startLoginMutation.isPending,
    isAddingAccount: startLoginMutation.isPending || pollingState === "polling",
    isRemovingAccount: removeAccountMutation.isPending,
    isSettingDefaultAccount: setDefaultAccountMutation.isPending,
    startAuth,
    addAccount: startAuth,
    cancelAuth,
    logout,
    removeAccount,
    setDefaultAccount,
    refetchStatus,
  };
}
