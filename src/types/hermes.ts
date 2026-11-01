export interface HermesModelConfig {
  default?: string;
  provider?: string;
  base_url?: string;
  context_length?: number;
  max_tokens?: number;
  [key: string]: unknown;
}

export type HermesMemoryKind = "memory" | "user";

export interface HermesMemoryLimits {
  memory: number;
  user: number;
  memoryEnabled: boolean;
  userEnabled: boolean;
}
