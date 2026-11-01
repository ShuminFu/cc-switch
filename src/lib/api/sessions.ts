import { invoke, call } from "./invoke";
import { commands } from "@/lib/bindings";
import type { SessionMessage, SessionMeta } from "@/types";

/** One bounded page of a transcript (`get_session_messages_page`). */
export interface SessionMessagePage {
  messages: SessionMessage[];
  /** Index of `messages[0]` in the whole transcript. */
  offset: number;
  total: number;
  hasMore: boolean;
  /** Transcript indices whose content was cut at `maxContentBytes`. */
  truncated: number[];
}

/** Token and cost totals of one session (`get_session_usage_summaries`). */
export interface SessionUsageSummary {
  sessionId: string;
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  totalCostUsd: number;
  models: string[];
  firstRequestAt?: number | null;
  lastRequestAt?: number | null;
}

/** Table-of-contents entry (`list_session_turns`). */
export interface SessionTurn {
  index: number;
  role: string;
  ts?: number;
  preview: string;
  contentBytes: number;
}

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

  async getMessagesPage(
    providerId: string,
    sourcePath: string,
    options: { offset?: number; limit?: number; maxContentBytes?: number } = {},
  ): Promise<SessionMessagePage> {
    return await invoke("get_session_messages_page", {
      providerId,
      sourcePath,
      ...options,
    });
  },

  /** Usage per session id; ids without usage are absent. */
  async getUsageSummaries(
    sessionIds: string[],
  ): Promise<Record<string, SessionUsageSummary>> {
    return await invoke("get_session_usage_summaries", { sessionIds });
  },

  async listTurns(
    providerId: string,
    sourcePath: string,
  ): Promise<SessionTurn[]> {
    return await invoke("list_session_turns", { providerId, sourcePath });
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
