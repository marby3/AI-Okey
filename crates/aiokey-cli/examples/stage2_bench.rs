//! 段2の採点時間を測る（段1の候補をそのまま渡す）。
//!
//!     cargo run --release -p aiokey-cli --example stage2_bench -- build/aiokey.dic data/models/zenz-v2.5-xsmall-q8_0.gguf
use std::time::Instant;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_scorer::{Config, PromptFormat, Request, Scorer};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dict = Dictionary::open(args[1].as_ref()).unwrap();
    if std::env::var_os("SYSINFO").is_some() {
        println!("{}", aiokey_scorer::system_info());
        return;
    }
    for format in [PromptFormat::ZenzV25, PromptFormat::Aiokey] {
        let threads = std::env::var("THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let mut scorer = Scorer::open(
            args[2].as_ref(),
            &Config {
                format,
                threads,
                ..Config::default()
            },
        )
        .unwrap();
        for input in ["kik", "htsthqtun", "knkeyboardds", "tqkgyknbsqphnikq"] {
            let k = std::env::var("K")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(32);
            let cands = convert(&dict, input, &Options { k });
            let pairs: Vec<(&str, &str)> = cands
                .iter()
                .map(|c| (c.surface.as_str(), c.reading.as_str()))
                .collect();
            let req = Request {
                context: "",
                input,
                candidates: pairs,
            };
            let mut times = Vec::new();
            let mut tokens = 0;
            for _ in 0..5 {
                let t = Instant::now();
                tokens = scorer.score(&req).unwrap().evaluated_tokens;
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(|a, b| a.total_cmp(b));
            println!(
                "{format:?} {input}: {} cands, {tokens} tokens, median {:.1} ms",
                req.candidates.len(),
                times[2]
            );
        }
    }
}
