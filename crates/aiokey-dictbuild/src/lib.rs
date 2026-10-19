//! Mozc の辞書（dictionary_oss）から、スケルトン索引辞書を作る。

use std::path::Path;

/// `dir` にある Mozc の辞書と接続コストから、辞書ファイルの中身を作る。
pub fn build_from_mozc(_dir: &Path) -> Result<Vec<u8>, String> {
    unimplemented!()
}

/// connection_single_column.txt を読み、品詞の数と N×N のコスト（行が rid、列が lid）を返す。
pub fn read_connection(_path: &Path) -> Result<(u16, Vec<i16>), String> {
    unimplemented!()
}
