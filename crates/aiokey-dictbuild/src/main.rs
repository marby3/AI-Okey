//! aiokey-dictbuild --mozc <dir> --out <file>
//!
//! Mozc の辞書からスケルトン索引辞書を作り、部分ごとの大きさを表示する。

use std::path::PathBuf;
use std::process::ExitCode;

use aiokey_dictbuild::build_from_mozc_with_report;
use aiokey_engine::dict::Dictionary;

fn main() -> ExitCode {
    let mut mozc = PathBuf::from("data/mozc");
    let mut out = PathBuf::from("build/aiokey.dic");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--mozc", Some(v)) => mozc = v.into(),
            ("--out", Some(v)) => out = v.into(),
            _ => {
                eprintln!("usage: aiokey-dictbuild [--mozc DIR] [--out FILE]");
                return ExitCode::from(2);
            }
        }
    }

    let (bytes, report) = match build_from_mozc_with_report(&mozc) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&out, &bytes) {
        eprintln!("error: {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    // 開く時間は examples/open_bench.rs で測る（作った直後はメモリを使い切っていて、正しく測れない）
    match Dictionary::open(&out) {
        Ok(dict) => {
            println!(
                "wrote {} ({:.1} MB)",
                out.display(),
                dict.size_bytes() as f64 / 1e6
            );
            println!(
                "  entries {} / skipped: empty skeleton {}, too long {}, bad context id {}",
                report.entries,
                report.skipped_empty_skeleton,
                report.skipped_too_long,
                report.skipped_bad_context_id
            );
            for (name, len) in dict.section_sizes() {
                println!("  {name:<11} {:>10.1} KB", len as f64 / 1e3);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: the written dictionary does not open: {e}");
            ExitCode::FAILURE
        }
    }
}
