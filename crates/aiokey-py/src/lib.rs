//! aiokey-engine を Python から呼ぶための拡張モジュール（`aiokey._engine`）。

use pyo3::prelude::*;

/// 読み（かな）をスケルトンに変える。
#[pyfunction]
fn skeletonize(reading: &str) -> String {
    aiokey_engine::skeleton::skeletonize(reading)
}

#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(skeletonize, m)?)?;
    Ok(())
}
