import type { TemplateType } from "@/config/constants";

export type ProviderCategory =
  | "official" // 官方
  | "cn_official" // 开源官方（原"国产官方"）
  | "cloud_provider" // 云服务商（AWS Bedrock 等）
  | "aggregator" // 聚合网站
  | "third_party" // 第三方供应商
  | "custom" // 自定义
  | "omo" // Oh My OpenCode
  | "omo-slim"; // Oh My OpenCode Slim

export interface Provider {
  id: string;
  name: string;
  settingsConfig: Record<string, unknown>; // 应用配置对象：Claude 为 settings.json；Codex 为 { auth, config }
  websiteUrl?: string;
  // 新增：供应商分类（用于差异化提示/能力开关）
  category?: ProviderCategory;
  createdAt?: number; // 添加时间戳（毫秒）
  sortIndex?: number; // 排序索引（用于自定义拖拽排序）
  // 备注信息
  notes?: string;
  // 新增：是否为商业合作伙伴
  isPartner?: boolean;
  // 可选：供应商元数据（仅存于 ~/.cc-switch/config.json，不写入 live 配置）
  meta?: ProviderMeta;
  // 图标配置
  icon?: string; // 图标名称（如 "openai", "anthropic"）
  iconColor?: string; // 图标颜色（Hex 格式，如 "#00A67E"）
  // 是否加入故障转移队列
  inFailoverQueue?: boolean;
}

export interface AppConfig {
  providers: Record<string, Provider>;
  current: string;
}

// 自定义端点配置
export interface CustomEndpoint {
  url: string;
  addedAt: number;
  lastUsed?: number;
}

// 端点候选项（用于端点测速弹窗）
export interface EndpointCandidate {
  id?: string;
  url: string;
  isCustom?: boolean;
}

// 用量查询脚本配置
export interface UsageScript {
  enabled: boolean; // 是否启用用量查询
  language: "javascript"; // 脚本语言
  code: string; // 脚本代码（JSON 格式配置）
  timeout?: number; // 超时时间（秒，默认 10）
  templateType?: TemplateType; // 模板类型（用于后端判断验证规则）
  apiKey?: string; // 用量查询专用的 API Key（通用模板使用）
  baseUrl?: string; // 用量查询专用的 Base URL（通用和 NewAPI 模板使用）
  accessToken?: string; // 访问令牌（NewAPI 模板使用）
  userId?: string; // 用户ID（NewAPI 模板使用）
  accessKeyId?: string; // 火山方舟 AccessKey ID（用量查询签名用，与推理 Key 分离）
  secretAccessKey?: string; // 火山方舟 SecretAccessKey
  teamOrganizationId?: string; // 智谱团队套餐组织 ID（请求头 bigmodel-organization）
  teamProjectId?: string; // 智谱团队套餐项目 ID（请求头 bigmodel-project）
  codingPlanProvider?: string; // Coding Plan 供应商标识（如 "kimi", "zhipu", "minimax"）
  autoQueryInterval?: number; // 自动查询间隔（单位：分钟，0 表示禁用）
  autoIntervalMinutes?: number; // 自动查询间隔（分钟）- 别名字段
  request?: {
    // 请求配置
    url?: string; // 请求 URL
    method?: string; // HTTP 方法
    headers?: Record<string, string>; // 请求头
    body?: unknown; // 请求体
  };
}

const DEFAULT_USAGE_SCRIPT: UsageScript = {
  enabled: false,
  language: "javascript",
  code: "",
  timeout: 10,
  autoQueryInterval: 5,
};

export function createUsageScript(
  overrides?: Partial<UsageScript>,
): UsageScript {
  return { ...DEFAULT_USAGE_SCRIPT, ...overrides };
}

// 单个套餐用量数据
export interface UsageData {
  planName?: string; // 套餐名称（可选）
  extra?: string; // 扩展字段，可自由补充需要展示的文本（可选）
  isValid?: boolean; // 套餐是否有效（可选）
  invalidMessage?: string; // 失效原因说明（可选，当 isValid 为 false 时显示）
  total?: number; // 总额度（可选）
  used?: number; // 已用额度（可选）
  remaining?: number; // 剩余额度（可选）
  unit?: string; // 单位（可选）
}

// 用量查询结果（支持多套餐）
export interface UsageResult {
  success: boolean;
  data?: UsageData[]; // 改为数组，支持返回多个套餐
  error?: string;
}

export type AuthBindingSource = "provider_config" | "managed_account";

export interface AuthBinding {
  source: AuthBindingSource;
  authProvider?: string;
  accountId?: string;
}

export interface ClaudeDesktopModelRoute {
  model: string;
  labelOverride?: string;
  supports1m?: boolean;
}

export type CodexChatThinkingParam =
  | "none"
  | "thinking"
  | "enable_thinking"
  | "reasoning_split";

export type CodexChatEffortParam =
  | "none"
  | "reasoning_effort"
  // OpenRouter 原生归一化对象 reasoning:{effort}（区别于顶层 OpenAI 别名 reasoning_effort）
  | "reasoning.effort";

export type CodexChatEffortValueMode =
  | "passthrough"
  | "low_high"
  | "deepseek"
  // OpenRouter effort 枚举 xhigh|high|medium|low|minimal（无 max，max 钳到 xhigh）
  | "openrouter";

export type CodexChatReasoningOutputFormat =
  | "auto"
  | "reasoning_content"
  | "reasoning"
  | "reasoning_details"
  | "think_tags";

export interface CodexChatReasoning {
  supportsThinking?: boolean;
  supportsEffort?: boolean;
  thinkingParam?: CodexChatThinkingParam;
  effortParam?: CodexChatEffortParam;
  effortValueMode?: CodexChatEffortValueMode;
  // 声明性字段：标注上游 reasoning 回传位置。当前提取靠穷举字段，未读取此值（think_tags 尚未接线）。
  outputFormat?: CodexChatReasoningOutputFormat;
}

export type PromptCacheRoutingMode = "auto" | "enabled" | "disabled";

export interface LocalProxyRequestOverrides {
  headers?: Record<string, string>;
  body?: Record<string, unknown>;
}

/**
 * Per-provider metadata stored next to `settingsConfig` (FE-PROV-38). Field
 * names match the backend (`ProviderMeta` in Rust); a few keep snake_case.
 * "Apps" lists where a field is meaningful; the provider form only persists
 * a field for those apps (see `src/components/providers/forms/adapters`).
 * 字段名与后端一致；仅在列出的应用中有意义。
 */
export interface ProviderMeta {
  /** Custom endpoints keyed by URL (all apps; endpoint speed test). 自定义端点 */
  custom_endpoints?: Record<string, CustomEndpoint>;
  /** Apply the shared common-config snippet on switch/live sync (Claude, Codex, Gemini). 通用配置 */
  commonConfigEnabled?: boolean;
  /** Claude Desktop 3P write mode: write the config directly or via the local proxy. */
  claudeDesktopMode?: "direct" | "proxy";
  /** Claude Desktop proxy mode: Claude-safe route name -> upstream model. */
  claudeDesktopModelRoutes?: Record<string, ClaudeDesktopModelRoute>;
  /** Usage/quota query script (all apps). 用量查询脚本 */
  usage_script?: UsageScript;
  /** Pick the fastest endpoint after a speed test (all apps). 自动选择端点 */
  endpointAutoSelect?: boolean;
  /** Created from a sponsored preset; not copied on duplicate. 合作伙伴 */
  isPartner?: boolean;
  /** Sponsor promotion key from the preset; not copied on duplicate. */
  partnerPromotionKey?: string;
  /** Cost multiplier applied to usage pricing (string decimal). 成本倍率 */
  costMultiplier?: string;
  /** Which model name prices requests (`request` / `response` ...). 计费模型来源 */
  pricingModelSource?: string;
  /**
   * Upstream API format for proxy conversion (Claude, Codex):
   * - `anthropic`: native Messages API, passed through
   * - `openai_chat`: OpenAI Chat Completions, converted
   * - `openai_responses`: OpenAI Responses, converted
   * - `gemini_native`: Gemini generateContent, converted
   */
  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native";
  /** Managed account binding (GitHub Copilot / Codex OAuth). 认证绑定 */
  authBinding?: AuthBinding;
  /**
   * Env variable that holds the Claude API key. Persisted only when it is not
   * the default `ANTHROPIC_AUTH_TOKEN`; reads/writes prefer it (FE-ARCH-47).
   */
  apiKeyField?: ClaudeApiKeyField;
  /** Treat `base_url` as the full endpoint; the proxy appends no path. */
  isFullUrl?: boolean;
  /** Prompt-cache key for OpenAI Responses-compatible endpoints; not copied on duplicate. */
  promptCacheKey?: string;
  /**
   * Session-based prompt-cache routing for Codex Responses -> Chat
   * conversions. `auto` enables it only for known-compatible upstreams.
   */
  promptCacheRouting?: PromptCacheRoutingMode;
  /** Codex OAuth FAST mode: injects `service_tier="priority"`. */
  codexFastMode?: boolean;
  /** Codex Responses -> Chat Completions reasoning capability metadata. */
  codexChatReasoning?: CodexChatReasoning;
  /** Codex -> Anthropic path: emulate the Claude Code client (only an explicit `true` enables it). */
  impersonateClaudeCode?: boolean;
  /**
   * Codex -> Anthropic path: Anthropic `max_tokens` override. Codex does not
   * forward `model_max_output_tokens`; without this the path falls back to a
   * conservative 8192. When set (>0) it wins over the request and default.
   */
  maxOutputTokens?: number;
  /** Custom User-Agent, applied by the local proxy only. */
  customUserAgent?: string;
  /** Header/body overrides, applied by the local proxy after route transforms. */
  localProxyRequestOverrides?: LocalProxyRequestOverrides;
  /** Special provider kind (`github_copilot`, `codex_oauth`, ...). 供应商类型 */
  providerType?: string;
  /** @deprecated Legacy GitHub Copilot account id; read for compatibility, use `authBinding`. */
  githubAccountId?: string;
}

// Skill 同步方式
export type SkillSyncMethod = "auto" | "symlink" | "copy";

// Skill 存储位置
export type SkillStorageLocation = "cc_switch" | "unified";

// Claude API 格式类型
// - "anthropic": 原生 Anthropic Messages API 格式，直接透传
// - "openai_chat": OpenAI Chat Completions 格式，需要格式转换
// - "openai_responses": OpenAI Responses API 格式，需要格式转换
// - "gemini_native": Gemini Native generateContent API 格式，需要格式转换
export type ClaudeApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native";

// Codex API 格式类型
// - "openai_responses": OpenAI Responses API 格式，直接透传
// - "openai_chat": OpenAI Chat Completions 格式，需要本地路由转换
// - "anthropic": native Anthropic Messages format, needs local routing to convert to Responses
export type CodexApiFormat = "openai_responses" | "openai_chat" | "anthropic";

export interface CodexCatalogModel {
  model: string;
  displayName?: string;
  contextWindow?: string | number;
  // Hidden provider capability metadata for the generated model catalog.
  // supportsParallelToolCalls is native-profile-only; inputModalities wins over
  // automatic text-only model detection for every profile.
  supportsParallelToolCalls?: boolean;
  inputModalities?: string[];
  // Vendor's OFFICIAL base_instructions (model identity / system preamble).
  // Codex requires this field in every catalog entry; when omitted the backend
  // falls back to a neutral default. e.g. MiMo "developed by Xiaomi".
  baseInstructions?: string;
}

// Claude 认证字段类型
export type ClaudeApiKeyField = "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";
