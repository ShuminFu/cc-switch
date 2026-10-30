export interface ProxyConfig {
  listen_address: string;
  listen_port: number;
  max_retries: number;
  request_timeout: number;
  enable_logging: boolean;
  live_takeover_active?: boolean;
  // 超时配置
  streaming_first_byte_timeout: number;
  streaming_idle_timeout: number;
  non_streaming_timeout: number;
}

// 与后端 DTO 一一对应，由 tauri-specta 生成（src/shared/bindings.ts）
export type { ActiveTarget, ProxyStatus } from "@/shared/bindings";

export interface ProxyServerInfo {
  address: string;
  port: number;
  started_at: string;
}

export interface ProxyTakeoverStatus {
  claude: boolean;
  "claude-desktop"?: boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
}

export interface ProviderHealth {
  provider_id: string;
  app_type: string;
  is_healthy: boolean;
  consecutive_failures: number;
  last_success_at: string | null;
  last_failure_at: string | null;
  last_error: string | null;
  updated_at: string;
}

// 熔断器相关类型
export interface CircuitBreakerConfig {
  failureThreshold: number;
  successThreshold: number;
  timeoutSeconds: number;
  errorRateThreshold: number;
  minRequests: number;
}

export type CircuitState = "closed" | "open" | "half_open";

export interface CircuitBreakerStats {
  state: CircuitState;
  consecutiveFailures: number;
  consecutiveSuccesses: number;
  totalRequests: number;
  failedRequests: number;
}

// 供应商健康状态枚举
export enum ProviderHealthStatus {
  Healthy = "healthy",
  Degraded = "degraded",
  Failed = "failed",
  Unknown = "unknown",
}

// 扩展 ProviderHealth 以包含前端计算的状态
export interface ProviderHealthWithStatus extends ProviderHealth {
  status: ProviderHealthStatus;
  circuitState?: CircuitState;
}

export interface ProxyUsageRecord {
  provider_id: string;
  app_type: string;
  endpoint: string;
  request_tokens: number | null;
  response_tokens: number | null;
  status_code: number;
  latency_ms: number;
  error: string | null;
  timestamp: string;
}

// 故障转移队列条目
export interface FailoverQueueItem {
  providerId: string;
  providerName: string;
  providerNotes?: string;
  sortIndex?: number;
}

// 全局代理配置（统一字段，三行镜像）
export interface GlobalProxyConfig {
  proxyEnabled: boolean;
  listenAddress: string;
  listenPort: number;
  enableLogging: boolean;
}

// 应用级代理配置（每个 app 独立）
export interface AppProxyConfig {
  appType: string;
  enabled: boolean;
  autoFailoverEnabled: boolean;
  maxRetries: number;
  streamingFirstByteTimeout: number;
  streamingIdleTimeout: number;
  nonStreamingTimeout: number;
  circuitFailureThreshold: number;
  circuitSuccessThreshold: number;
  circuitTimeoutSeconds: number;
  circuitErrorRateThreshold: number;
  circuitMinRequests: number;
}

// 缺陷报告抓包状态（CCS-138）
export interface CaptureStatus {
  active: boolean;
  // 自动关闭时间（Unix 毫秒）
  expiresAt: number | null;
  remainingSecs: number;
  // 可导出的请求 ID（最新的在前）
  capturedRequestIds: string[];
}

// 请求追踪（CCS-139）
export interface TraceHeader {
  name: string;
  value: string;
}

export interface TraceRoutePlan {
  client: string;
  upstream: string;
  apiFormat: string | null;
  decode: string;
  url: string;
  auth: string | null;
  quirks: string[];
  sideStores: string[];
}

export type TraceAttemptStatus =
  | "in_flight"
  | "succeeded"
  | "failed"
  | "skipped";

export interface TraceAttempt {
  providerId: string;
  providerName: string;
  status: TraceAttemptStatus;
  // 失败或跳过的原因（已脱敏）
  reason: string | null;
  upstreamStatus: number | null;
  plan: TraceRoutePlan | null;
  // 按顺序应用的恢复策略
  recovery: string[];
  upstreamHeaders: TraceHeader[];
  // 相对请求开始的毫秒数
  startedMs: number;
  durationMs: number | null;
}

export type TraceUsageSource = "stream_events" | "response_body" | "missing";

export interface RequestTrace {
  traceId: string;
  requestId: string | null;
  // Unix 毫秒
  startedAt: number;
  appType: string;
  method: string;
  path: string;
  isStream: boolean;
  clientHeaders: TraceHeader[];
  attempts: TraceAttempt[];
  droppedAttempts: number;
  statusCode: number | null;
  error: string | null;
  ttfbMs: number | null;
  firstTokenMs: number | null;
  latencyMs: number | null;
  usageSource: TraceUsageSource | null;
}
