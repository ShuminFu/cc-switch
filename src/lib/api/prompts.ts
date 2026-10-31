import { invoke, call } from "./invoke";
import { commands } from "@/lib/bindings";
import type { AppId } from "./types";

export interface Prompt {
  id: string;
  name: string;
  content: string;
  description?: string;
  enabled: boolean;
  createdAt?: number;
  updatedAt?: number;
}

export const promptsApi = {
  async getPrompts(app: AppId): Promise<Record<string, Prompt>> {
    return await invoke("get_prompts", { app });
  },

  async upsertPrompt(app: AppId, id: string, prompt: Prompt): Promise<void> {
    return await invoke("upsert_prompt", { app, id, prompt });
  },

  async deletePrompt(app: AppId, id: string): Promise<void> {
    return await call(commands.deletePrompt(app, id));
  },

  async enablePrompt(app: AppId, id: string): Promise<void> {
    return await call(commands.enablePrompt(app, id));
  },

  async importFromFile(app: AppId): Promise<string> {
    return await call(commands.importPromptFromFile(app));
  },

  async getCurrentFileContent(app: AppId): Promise<string | null> {
    return await call(commands.getCurrentPromptFileContent(app));
  },
};
