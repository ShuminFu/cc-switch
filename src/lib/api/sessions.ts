import { invoke } from "@tauri-apps/api/core";
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

export type TranscriptFormat = "markdown" | "json";

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
    return await invoke("delete_session", {
      providerId,
      sessionId,
      sourcePath,
    });
  },

  async deleteMany(
    items: DeleteSessionOptions[],
  ): Promise<DeleteSessionResult[]> {
    return await invoke("delete_sessions", { items });
  },

  /** Save dialog filtered to the transcript format; null when cancelled */
  async saveTranscriptDialog(
    defaultName: string,
    format: TranscriptFormat,
  ): Promise<string | null> {
    return await invoke("save_session_transcript_dialog", {
      defaultName,
      format,
    });
  },

  /** Render the session's messages and write them to targetPath; resolves to the written path */
  async exportTranscript(options: {
    session: SessionMeta;
    targetPath: string;
    format: TranscriptFormat;
  }): Promise<string> {
    return await invoke("export_session_transcript", options);
  },

  /** Open a project directory, or reveal a source file, in the system file manager */
  async revealPath(path: string): Promise<void> {
    await invoke("reveal_session_path", { path });
  },

  async launchTerminal(options: {
    command: string;
    cwd?: string | null;
    customConfig?: string | null;
  }): Promise<boolean> {
    const { command, cwd, customConfig } = options;
    return await invoke("launch_session_terminal", {
      command,
      cwd,
      customConfig,
    });
  },
};
