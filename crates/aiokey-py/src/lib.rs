//! aiokey-engine を Python から呼ぶための拡張モジュール（`aiokey._engine`）。
//! データ生成と評価に使う。

use std::path::Path;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;

/// 読み（かな）をスケルトンに変える。
#[pyfunction]
fn skeletonize(reading: &str) -> String {
    aiokey_engine::skeleton::skeletonize(reading)
}

/// 読みを（スケルトンの文字, かなの母音または None）の並びに分ける。
#[pyfunction]
fn units(reading: &str) -> Vec<(String, Option<String>)> {
    aiokey_engine::skeleton::units(reading)
        .into_iter()
        .map(|(letter, vowel)| (letter.to_string(), vowel.map(|v| v.to_string())))
        .collect()
}

/// Mozc のデータ（`mozc_dir`）から辞書を作り、`out` に書く。
#[pyfunction]
fn build_dictionary(mozc_dir: &str, out: &str) -> PyResult<()> {
    let bytes =
        aiokey_dictbuild::build_from_mozc(Path::new(mozc_dir)).map_err(PyValueError::new_err)?;
    std::fs::write(out, bytes).map_err(|e| PyOSError::new_err(format!("{out}: {e}")))
}

/// 英単語の頻度 TSV を、辞書ビルダーと同じ解析で読む。
#[pyfunction]
fn read_english_tsv(path: &str) -> PyResult<Vec<(String, u32)>> {
    aiokey_dictbuild::read_english_tsv(Path::new(path)).map_err(PyValueError::new_err)
}

/// 辞書を開いて段1の変換をする。
#[pyclass(frozen)]
struct Converter {
    dict: Dictionary,
}

#[pymethods]
impl Converter {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let dict =
            Dictionary::open(Path::new(path)).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { dict })
    }

    /// 上位 `k` 件の候補の表記を、コストの低い順に返す。
    #[pyo3(signature = (input, k = 32))]
    fn convert(&self, py: Python<'_>, input: &str, k: usize) -> Vec<String> {
        py.allow_threads(|| {
            convert(&self.dict, input, &Options { k })
                .into_iter()
                .map(|c| c.surface)
                .collect()
        })
    }
}

#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(skeletonize, m)?)?;
    m.add_function(wrap_pyfunction!(units, m)?)?;
    m.add_function(wrap_pyfunction!(build_dictionary, m)?)?;
    m.add_function(wrap_pyfunction!(read_english_tsv, m)?)?;
    m.add_class::<Converter>()?;
    Ok(())
}
