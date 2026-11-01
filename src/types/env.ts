/**
 * 环境变量冲突检测相关类型定义
 */

/**
 * 环境变量冲突信息
 */
export interface EnvConflict {
  /** 环境变量名称 */
  varName: string;
  /** 环境变量的值 */
  varValue: string;
  /** 来源类型: "system" 表示系统环境变量, "file" 表示配置文件 */
  sourceType: "system" | "file";
  /** 来源路径 (注册表路径或文件路径:行号) */
  sourcePath: string;
}

/**
 * 备份信息
 */
export interface BackupInfo {
  /** 备份文件路径 */
  backupPath: string;
  /** 备份时间戳 */
  timestamp: string;
  /** 被备份的环境变量冲突列表 */
  conflicts: EnvConflict[];
  /**
   * Conflicts that were not removed because cc-switch cannot change their
   * source (process environment from launchd/systemd/the parent shell, SVC-48).
   */
  skipped?: SkippedEnv[];
}

/** A conflict the delete left in place (`SkippedEnv` in env_manager.rs). */
export interface SkippedEnv {
  varName: string;
  sourceType: string;
  sourcePath: string;
  /** Stable reason code, e.g. `process_environment_unsupported`. */
  reason: string;
}

/** Output syntax of `render_provider_env_profile`. */
export type EnvShell = "bash" | "fish" | "powershell" | "dotenv";
