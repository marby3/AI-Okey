//! AI-O Key の変換サーバー。
//!
//! `service` は OS に依存しない gRPC サービスの実装、`config` は設定（TOML）、
//! `pipe` は Windows の名前付きパイプでの待ち受け。

pub mod config;
pub mod service;

#[cfg(windows)]
mod pipe;
#[cfg(windows)]
pub use pipe::TonicNamedPipeServer;
