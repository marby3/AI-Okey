//! AI-O Key の変換を試す・測るための CLI。
//!
//!     aiokey-cli convert <入力> [--dict FILE] [-k N]
//!     aiokey-cli bench [--dict FILE] [--iterations N] [--stage2 GGUF [--threads N] [--temperature T]]
//!
//! bench に --stage2 を渡すと、段1の上位 8 件（サーバーの既定と同じ）を段2で採点する時間も測る（#14、実機での計測用）。
//! 辞書の既定は build/aiokey.dic（aiokey-dictbuild の出力）。

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_engine::skeleton::skeletonize;
use aiokey_scorer::{Config as ScorerConfig, PromptFormat, Request, Scorer};

/// 段2で採点する候補の数（aiokey-server の stage2.max_candidates の既定と同じ）
const STAGE2_K: usize = 8;

/// bench で変換する文。読みをスケルトンにしたものを入力にする（20 文字前後）。
const BENCH_READINGS: &[&str] = &[
    "はたしてほんとうにそれでよかったのだろうか",
    "きのうはともだちといっしょにえいがをみにいった",
    "あしたのかいぎはじゅうじからにかいでおこないます",
    "このしょうひんはわかいひとのあいだでにんきがある",
    "わたしはまいにちにほんごをべんきょうしています",
    "じこのえいきょうででんしゃがおくれているらしい",
    "しりょうはあとでめーるでおくりますのでごかくにんください",
    "てんきがよければしゅうまつはこうえんをさんぽしよう",
];

const USAGE: &str = "usage:
  aiokey-cli convert <input> [--dict FILE] [-k N]
  aiokey-cli bench [--dict FILE] [--iterations N] [--stage2 GGUF [--threads N] [--temperature T]]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut dict_path = PathBuf::from("build/aiokey.dic");
    let mut k = Options::default().k;
    let mut iterations = 100usize;
    let mut stage2: Option<PathBuf> = None;
    let mut threads = 2i32;
    let mut temperature = 1.0f32;
    let mut positional = Vec::new();
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        let value = match arg.as_str() {
            "--dict" | "-k" | "--iterations" | "--stage2" | "--threads" | "--temperature" => {
                match rest.next() {
                    Some(v) => v,
                    None => {
                        eprintln!("{arg} needs a value\n{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            _ => {
                positional.push(arg.clone());
                continue;
            }
        };
        match arg.as_str() {
            "--dict" => dict_path = value.into(),
            "-k" => k = value.parse().unwrap_or(k),
            "--stage2" => stage2 = Some(value.into()),
            "--threads" => threads = value.parse().unwrap_or(threads),
            "--temperature" => temperature = value.parse().unwrap_or(temperature),
            _ => iterations = value.parse().unwrap_or(iterations),
        }
    }

    let dict = match Dictionary::open(&dict_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {}: {e}", dict_path.display());
            return ExitCode::FAILURE;
        }
    };

    match command.as_str() {
        "convert" => {
            let Some(input) = positional.first() else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            for c in convert(&dict, input, &Options { k }) {
                let bunsetsu: Vec<String> = c
                    .bunsetsu
                    .iter()
                    .map(|r| format!("{}..{}", r.start, r.end))
                    .collect();
                println!(
                    "{}\t{}\t{}\t{}",
                    c.cost,
                    c.surface,
                    c.reading,
                    bunsetsu.join(",")
                );
            }
            ExitCode::SUCCESS
        }
        "bench" => {
            let inputs: Vec<String> = BENCH_READINGS.iter().map(|r| skeletonize(r)).collect();
            let mut scorer = match &stage2 {
                Some(path) => {
                    let config = ScorerConfig {
                        threads,
                        format: PromptFormat::Aiokey,
                        max_candidates: STAGE2_K,
                        temperature,
                    };
                    match Scorer::open(path, &config) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            eprintln!("error: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
                None => None,
            };
            let (mut stage1_ms, mut stage2_ms, mut total_ms) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..iterations.max(1) {
                for input in &inputs {
                    let t = Instant::now();
                    let candidates = convert(&dict, input, &Options { k });
                    let s1 = t.elapsed().as_secs_f64() * 1000.0;
                    stage1_ms.push(s1);
                    if let Some(scorer) = scorer.as_mut() {
                        let t = Instant::now();
                        let req = Request {
                            context: "",
                            input,
                            candidates: candidates
                                .iter()
                                .map(|c| (c.surface.as_str(), c.reading.as_str()))
                                .collect(),
                        };
                        if let Err(e) = scorer.score(&req) {
                            eprintln!("error: {e}");
                            return ExitCode::FAILURE;
                        }
                        let s2 = t.elapsed().as_secs_f64() * 1000.0;
                        stage2_ms.push(s2);
                        total_ms.push(s1 + s2);
                    }
                }
            }
            let avg_len = inputs.iter().map(|s| s.chars().count()).sum::<usize>() as f64
                / inputs.len() as f64;
            println!(
                "inputs {} (avg {avg_len:.1} chars), runs {}",
                inputs.len(),
                stage1_ms.len()
            );
            report("", &mut stage1_ms);
            if scorer.is_some() {
                println!("stage2: top {STAGE2_K} candidates, {threads} threads");
                println!("{}", aiokey_scorer::system_info());
                report("stage2 ", &mut stage2_ms);
                report("total ", &mut total_ms);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// 時間の p50、p95、最大を出す（段1だけのときは今までどおり接頭辞なし）。
fn report(prefix: &str, times: &mut [f64]) {
    times.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    println!("{prefix}p50 {:.2} ms", at(0.5));
    println!("{prefix}p95 {:.2} ms", at(0.95));
    println!("{prefix}max {:.2} ms", at(1.0));
}
