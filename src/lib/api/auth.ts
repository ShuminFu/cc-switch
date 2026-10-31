import { call } from "./invoke";
import { commands } from "@/lib/bindings";
import type * as Bindings from "@/lib/bindings";

export type ManagedAuthProvider = "github_copilot" | "codex_oauth";

// Shapes come from the Rust structs (src/lib/bindings.ts); `provider` is
// narrowed to the providers the frontend supports.
export type ManagedAuthAccount = Omit<
  Bindings.ManagedAuthAccount,
  "provider"
> & {
  provider: ManagedAuthProvider;
};

export type ManagedAuthStatus = Omit<
  Bindings.ManagedAuthStatus,
  "provider" | "accounts"
> & {
  provider: ManagedAuthProvider;
  accounts: ManagedAuthAccount[];
};

export type ManagedAuthDeviceCodeResponse = Omit<
  Bindings.ManagedAuthDeviceCodeResponse,
  "provider"
> & {
  provider: ManagedAuthProvider;
};

export async function authStartLogin(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
): Promise<ManagedAuthDeviceCodeResponse> {
  return (await call(
    commands.authStartLogin(authProvider, githubDomain || null),
  )) as ManagedAuthDeviceCodeResponse;
}

export async function authPollForAccount(
  authProvider: ManagedAuthProvider,
  deviceCode: string,
  githubDomain?: string,
): Promise<ManagedAuthAccount | null> {
  return (await call(
    commands.authPollForAccount(authProvider, deviceCode, githubDomain || null),
  )) as ManagedAuthAccount | null;
}

export async function authListAccounts(
  authProvider: ManagedAuthProvider,
): Promise<ManagedAuthAccount[]> {
  return (await call(
    commands.authListAccounts(authProvider),
  )) as ManagedAuthAccount[];
}

export async function authGetStatus(
  authProvider: ManagedAuthProvider,
): Promise<ManagedAuthStatus> {
  return (await call(
    commands.authGetStatus(authProvider),
  )) as ManagedAuthStatus;
}

export async function authRemoveAccount(
  authProvider: ManagedAuthProvider,
  accountId: string,
): Promise<void> {
  return call(commands.authRemoveAccount(authProvider, accountId));
}

export async function authSetDefaultAccount(
  authProvider: ManagedAuthProvider,
  accountId: string,
): Promise<void> {
  return call(commands.authSetDefaultAccount(authProvider, accountId));
}

export async function authLogout(
  authProvider: ManagedAuthProvider,
): Promise<void> {
  return call(commands.authLogout(authProvider));
}

export const authApi = {
  authStartLogin,
  authPollForAccount,
  authListAccounts,
  authGetStatus,
  authRemoveAccount,
  authSetDefaultAccount,
  authLogout,
};
