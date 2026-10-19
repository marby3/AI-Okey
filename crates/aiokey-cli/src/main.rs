//! AI-O Key の変換を試す・測るための CLI。
//!
//!     aiokey-cli convert <入力> [--dict FILE] [-k N]
//!     aiokey-cli bench [--dict FILE] [--iterations N]
//!
//! 辞書の既定は build/aiokey.dic（aiokey-dictbuild の出力）。

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_engine::skeleton::skeletonize;

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
  aiokey-cli bench [--dict FILE] [--iterations N]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut dict_path = PathBuf::from("build/aiokey.dic");
    let mut k = Options::default().k;
    let mut iterations = 100usize;
    let mut positional = Vec::new();
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        let value = match arg.as_str() {
            "--dict" | "-k" | "--iterations" => match rest.next() {
                Some(v) => v,
                None => {
                    eprintln!("{arg} needs a value\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ => {
                positional.push(arg.clone());
                continue;
            }
        };
        match arg.as_str() {
            "--dict" => dict_path = value.into(),
            "-k" => k = value.parse().unwrap_or(k),
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
            let mut times = Vec::new();
            for _ in 0..iterations.max(1) {
                for input in &inputs {
                    let t = Instant::now();
                    let _ = convert(&dict, input, &Options { k });
                    times.push(t.elapsed().as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(|a, b| a.total_cmp(b));
            let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
            let avg_len = inputs.iter().map(|s| s.chars().count()).sum::<usize>() as f64
                / inputs.len() as f64;
            println!(
                "inputs {} (avg {avg_len:.1} chars), runs {}",
                inputs.len(),
                times.len()
            );
            println!("p50 {:.2} ms", at(0.5));
            println!("p95 {:.2} ms", at(0.95));
            println!("max {:.2} ms", at(1.0));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
