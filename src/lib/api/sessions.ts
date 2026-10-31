import { invoke, call } from "./invoke";
import { commands } from "@/lib/bindings";
import type { SessionMessage, SessionMeta } from "@/types";

export interface DeleteSessionOptions {
  providerId: string;
  sessionId: string;
  sourcePath: string;
}

export interface DeleteSessionResult extends DeleteSessionOptions {
  success: boolean;
  error?: string;
}

export const sessionsApi = {
  async list(): Promise<SessionMeta[]> {
    return await invoke("list_sessions");
  },

  async getMessages(
    providerId: string,
    sourcePath: string,
  ): Promise<SessionMessage[]> {
    return await invoke("get_session_messages", { providerId, sourcePath });
  },

  async delete(options: DeleteSessionOptions): Promise<boolean> {
    const { providerId, sessionId, sourcePath } = options;
    return await call(
      commands.deleteSession(providerId, sessionId, sourcePath),
    );
  },

  async deleteMany(
    items: DeleteSessionOptions[],
  ): Promise<DeleteSessionResult[]> {
    return await invoke("delete_sessions", { items });
  },

  async launchTerminal(options: {
    providerId: string;
    sessionId: string;
    cwd?: string | null;
  }): Promise<boolean> {
    const { providerId, sessionId, cwd } = options;
    return await call(
      commands.launchSessionTerminal(providerId, sessionId, cwd ?? null),
    );
  },
};
