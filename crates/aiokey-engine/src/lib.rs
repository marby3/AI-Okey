//! AI-O Key の変換エンジン。OS に依存しないので、Windows の IME、Linux の CI、
//! Python のデータ生成（aiokey-py 経由）から同じものを使う。

pub mod convert;
pub mod dict;
pub mod input;
pub mod skeleton;
