import { call } from "./invoke";
import { commands } from "@/lib/bindings";
import type {
  DailyMemoryFileInfo,
  DailyMemorySearchResult,
} from "@/lib/bindings";
export type {
  DailyMemoryFileInfo,
  DailyMemorySearchResult,
} from "@/lib/bindings";

export const workspaceApi = {
  async readFile(filename: string): Promise<string | null> {
    return call(commands.readWorkspaceFile(filename));
  },

  async writeFile(filename: string, content: string): Promise<void> {
    return call(commands.writeWorkspaceFile(filename, content));
  },

  async listDailyMemoryFiles(): Promise<DailyMemoryFileInfo[]> {
    return call(commands.listDailyMemoryFiles());
  },

  async readDailyMemoryFile(filename: string): Promise<string | null> {
    return call(commands.readDailyMemoryFile(filename));
  },

  async writeDailyMemoryFile(filename: string, content: string): Promise<void> {
    return call(commands.writeDailyMemoryFile(filename, content));
  },

  async deleteDailyMemoryFile(filename: string): Promise<void> {
    return call(commands.deleteDailyMemoryFile(filename));
  },

  async searchDailyMemoryFiles(
    query: string,
  ): Promise<DailyMemorySearchResult[]> {
    return call(commands.searchDailyMemoryFiles(query));
  },

  async openDirectory(subdir: "workspace" | "memory"): Promise<void> {
    await call(commands.openWorkspaceDirectory(subdir));
  },
};
