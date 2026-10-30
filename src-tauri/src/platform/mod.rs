//! 平台相关的进程、shell 引号与终端启动原语。
//! 所有"拼 shell/batch 字符串并拉起外部进程"的逻辑集中在这里，便于统一审计引号与临时文件处理。
pub(crate) mod process;
pub(crate) mod shell;
pub(crate) mod terminal;
