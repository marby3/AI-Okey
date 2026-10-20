//! aiokey-engine を Python から呼ぶための拡張モジュール（`aiokey._engine`）。
//! データ生成と評価に使う。

use std::path::Path;

use std::sync::Mutex;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_scorer::{Config, PromptFormat, Request, Scorer as ScorerImpl};
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

    /// 上位 `k` 件の候補の (表記, 読み) を、コストの低い順に返す。
    #[pyo3(signature = (input, k = 32))]
    fn convert_with_readings(
        &self,
        py: Python<'_>,
        input: &str,
        k: usize,
    ) -> Vec<(String, String)> {
        py.allow_threads(|| {
            convert(&self.dict, input, &Options { k })
                .into_iter()
                .map(|c| (c.surface, c.reading))
                .collect()
        })
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

/// 段2の採点器（zenz の GGUF を llama.cpp で動かす）。
#[pyclass(frozen)]
struct Scorer {
    inner: Mutex<ScorerImpl>,
}

#[pymethods]
impl Scorer {
    /// `format` は "zenz"（zenz-v2.5 互換）か "aiokey"。
    #[new]
    #[pyo3(signature = (path, format = "zenz", threads = 2, max_candidates = 32, temperature = 1.0))]
    fn new(
        path: &str,
        format: &str,
        threads: i32,
        max_candidates: usize,
        temperature: f32,
    ) -> PyResult<Self> {
        let format = match format {
            "zenz" => PromptFormat::ZenzV25,
            "aiokey" => PromptFormat::Aiokey,
            other => return Err(PyValueError::new_err(format!("unknown format: {other}"))),
        };
        let config = Config {
            threads,
            format,
            max_candidates,
            temperature,
        };
        let inner = ScorerImpl::open(Path::new(path), &config).map_err(PyValueError::new_err)?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }

    /// 候補（(表記, 読み) の並び）を採点し、確率（候補と同じ並び）と margin を返す。
    fn score(
        &self,
        py: Python<'_>,
        context: &str,
        input: &str,
        candidates: Vec<(String, String)>,
    ) -> PyResult<(Vec<f32>, f32)> {
        py.allow_threads(|| {
            let req = Request {
                context,
                input,
                candidates: candidates
                    .iter()
                    .map(|(s, r)| (s.as_str(), r.as_str()))
                    .collect(),
            };
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| PyValueError::new_err("scorer is poisoned"))?;
            let scores = inner.score(&req).map_err(PyValueError::new_err)?;
            Ok((scores.probs, scores.margin))
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
    m.add_class::<Scorer>()?;
    Ok(())
}
