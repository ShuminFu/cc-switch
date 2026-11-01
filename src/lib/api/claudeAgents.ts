import { invoke } from "./invoke";

/** A Claude Code subagent file in `~/.claude/agents` (PRODUCT-42). */
export interface ClaudeAgentInfo {
  fileName: string;
  path: string;
  name: string;
  description?: string;
  model?: string;
  tools?: string;
  /** Markdown body: the agent's system prompt. */
  prompt: string;
  /** Frontmatter missing or incomplete; Claude Code may ignore the file. */
  invalid?: boolean;
}

export interface NewClaudeAgent {
  name: string;
  description: string;
  model?: string;
  /** Comma-separated tool names; empty inherits every tool. */
  tools?: string;
  prompt: string;
}

export const claudeAgentsApi = {
  list: () => invoke<ClaudeAgentInfo[]>("list_claude_agents"),
  /** Writes `~/.claude/agents/<name>.md`; never overwrites a file. */
  create: (agent: NewClaudeAgent) =>
    invoke<ClaudeAgentInfo>("create_claude_agent", { agent }),
};
