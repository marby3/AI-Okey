//! aiokey-engine を Python から呼ぶための拡張モジュール（`aiokey._engine`）。
//! データ生成と評価に使う。

use std::path::Path;

use std::sync::Mutex;

use aiokey_engine::convert::{convert, lattice_words, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_scorer::generate::{Constraint, GenerateOptions, Word};
use aiokey_scorer::{Config, PromptFormat, Request, Scorer as ScorerImpl, Scores};
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

/// 文の途中に `/` があるか（あれば段1のコスト差にかかわらず段2で並べ替える。#52）。
#[pyfunction]
fn has_middle_slash(input: &str) -> bool {
    aiokey_engine::input::has_middle_slash(input)
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

    /// 上位 `k` 件の候補の (表記, 読み, コスト) を、コストの低い順に返す（段2を飛ばすかの判定に使う。#39）。
    #[pyo3(signature = (input, k = 32))]
    fn convert_with_costs(
        &self,
        py: Python<'_>,
        input: &str,
        k: usize,
    ) -> Vec<(String, String, i32)> {
        py.allow_threads(|| {
            convert(&self.dict, input, &Options { k })
                .into_iter()
                .map(|c| (c.surface, c.reading, c.cost))
                .collect()
        })
    }

    /// 入力の長さと、段1のラティスに置く語の (開始位置, 終了位置, 表記, コスト) を返す（#40 の分析用）。
    fn lattice_words(&self, input: &str) -> (usize, Vec<(usize, usize, String, i32)>) {
        let (len, words) = lattice_words(&self.dict, input);
        (
            len,
            words
                .into_iter()
                .map(|w| (w.start, w.end, w.surface, w.cost))
                .collect(),
        )
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

fn prompt_format(name: &str) -> PyResult<PromptFormat> {
    match name {
        "zenz" => Ok(PromptFormat::ZenzV25),
        "aiokey" => Ok(PromptFormat::Aiokey),
        other => Err(PyValueError::new_err(format!("unknown format: {other}"))),
    }
}

/// 段2で候補の前に置くプロンプト（学習でも採点器と同じものを使うため）。
#[pyfunction]
#[pyo3(name = "prompt")]
fn build_prompt(format: &str, context: &str, input: &str, reading: &str) -> PyResult<String> {
    Ok(aiokey_scorer::prompt(
        prompt_format(format)?,
        context,
        input,
        reading,
    ))
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
        let format = prompt_format(format)?;
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
        let scores = self.run(py, context, input, candidates)?;
        Ok((scores.probs, scores.margin))
    }

    /// 段1のラティスの語で制約して、入力に合う文を生成する（#40 の試作）。
    /// (表記, 並べる値＝段2の log P − cost_weight × 段1のコスト ÷ 500) を高い順に返す。
    #[pyo3(signature = (converter, context, input, beam = 8, cost_weight = 0.5))]
    fn generate(
        &self,
        py: Python<'_>,
        converter: &Converter,
        context: &str,
        input: &str,
        beam: usize,
        cost_weight: f32,
    ) -> PyResult<Vec<(String, f32)>> {
        py.allow_threads(|| {
            let (len, words) = lattice_words(&converter.dict, input);
            let words: Vec<Word> = words
                .iter()
                .map(|w| Word {
                    start: w.start,
                    end: w.end,
                    surface: &w.surface,
                    cost: w.cost,
                })
                .collect();
            let constraint = Constraint::new(len, &words);
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| PyValueError::new_err("scorer is poisoned"))?;
            inner
                .generate(
                    context,
                    input,
                    &constraint,
                    &GenerateOptions { beam, cost_weight },
                )
                .map_err(PyValueError::new_err)
        })
    }

    /// 候補ごとの log P(候補 | 文脈, 入力) を返す（温度をかける前の値。学習側との答え合わせ用）。
    fn log_probs(
        &self,
        py: Python<'_>,
        context: &str,
        input: &str,
        candidates: Vec<(String, String)>,
    ) -> PyResult<Vec<f32>> {
        Ok(self.run(py, context, input, candidates)?.log_probs)
    }
}

impl Scorer {
    fn run(
        &self,
        py: Python<'_>,
        context: &str,
        input: &str,
        candidates: Vec<(String, String)>,
    ) -> PyResult<Scores> {
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
            inner.score(&req).map_err(PyValueError::new_err)
        })
    }
}

#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(skeletonize, m)?)?;
    m.add_function(wrap_pyfunction!(units, m)?)?;
    m.add_function(wrap_pyfunction!(has_middle_slash, m)?)?;
    m.add_function(wrap_pyfunction!(build_dictionary, m)?)?;
    m.add_function(wrap_pyfunction!(read_english_tsv, m)?)?;
    m.add_function(wrap_pyfunction!(build_prompt, m)?)?;
    m.add_class::<Converter>()?;
    m.add_class::<Scorer>()?;
    Ok(())
}
