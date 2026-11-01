import { getVersion } from "@tauri-apps/api/app";
import { commands } from "@/lib/bindings";
import { call } from "@/lib/api/invoke";

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
}

export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "";
  }
}

/**
 * Ask the backend's updater (`check_app_update`, CMD-39). The webview no
 * longer talks to the updater plugin directly, so it needs no updater grant.
 */
export async function checkForUpdate(
  opts: CheckOptions = {},
): Promise<
  { status: "up-to-date" } | { status: "available"; info: UpdateInfo }
> {
  const update = await call(commands.checkAppUpdate(opts.timeout ?? 30000));

  if (!update) {
    return { status: "up-to-date" };
  }

  const info: UpdateInfo = {
    currentVersion: update.currentVersion || (await getCurrentVersion()),
    availableVersion: update.availableVersion,
    notes: update.notes ?? undefined,
    pubDate: update.pubDate ?? undefined,
  };

  return { status: "available", info };
}
