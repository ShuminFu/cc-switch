//! 模型定价
//!
//! - [`catalog`]：内置定价目录（`catalog.json`），启动时同步到 `model_pricing` 表
//! - [`resolver`]：把各种模型名归一化后在 `model_pricing` 表中查价
//! - [`engine`]：唯一的成本函数 [`engine::cost`]，ingest 与回填共用

pub mod catalog;
pub mod engine;
pub mod resolver;
