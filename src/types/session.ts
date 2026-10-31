export interface SessionMeta {
  providerId: string;
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string | null;
  createdAt?: number;
  lastActiveAt?: number;
  sourcePath?: string;
  resumeCommand?: string;
  /** Typed source (SESS-20); `sourcePath` is its legacy string form. */
  source?: SessionSource;
  /** Server-computed stable key. */
  sessionKey?: string;
}

export type SessionSource =
  | { kind: "file"; path: string }
  | { kind: "dir"; path: string }
  | { kind: "sqlite"; db: string; id: string };

export interface SessionMessage {
  role: string;
  content: string;
  ts?: number;
}
