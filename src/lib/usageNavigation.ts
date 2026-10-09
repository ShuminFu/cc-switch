/**
 * 跨页面的「查看该会话的请求」导航：会话管理器写入待应用的会话 ID，
 * 用量页的请求日志表挂载时取走并作为筛选；已挂载时通过订阅即时更新。
 */
let pendingSessionId: string | null = null;
const listeners = new Set<(sessionId: string) => void>();

export function requestLogSessionFilter(sessionId: string): void {
  const trimmed = sessionId.trim();
  if (!trimmed) return;
  pendingSessionId = trimmed;
  listeners.forEach((listener) => listener(trimmed));
}

/** 取走待应用的会话筛选（只消费一次） */
export function takePendingSessionFilter(): string | null {
  const value = pendingSessionId;
  pendingSessionId = null;
  return value;
}

export function subscribeSessionFilter(
  listener: (sessionId: string) => void,
): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 测试用：清空状态 */
export function resetSessionFilterForTests(): void {
  pendingSessionId = null;
  listeners.clear();
}
