export interface OmoLocalFileData {
  agents?: Record<string, Record<string, unknown>>;
  categories?: Record<string, Record<string, unknown>>;
  otherFields?: Record<string, unknown>;
  filePath: string;
  lastModified?: string;
}

export interface OmoAgentDef {
  key: string;
  display: string;
  descKey: string;
  tooltipKey: string;
  recommended?: string;
  group: "main" | "sub";
}

export interface OmoCategoryDef {
  key: string;
  display: string;
  descKey: string;
  tooltipKey: string;
  recommended?: string;
}
