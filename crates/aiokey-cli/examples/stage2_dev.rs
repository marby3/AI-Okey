//! 段2の閾値を決めるための計測（#13）。例ごとに、段1の確信度、段2の margin、正解したか、かかった時間を出す。
//!
//!     cargo run --release -p aiokey-cli --example stage2_dev -- <aiokey.dic> <段2の GGUF> <温度> <例の TSV>
//!
//! 例の TSV は「入力 \t 文脈 \t 正解」の行（python 側で aiokey.datagen の JSONL から作る）。
//! 環境変数：THREADS（推論のスレッド数、既定 2）、STAGE2_K（段2で採点する候補の上限、既定 32）。
//! 出力の TSV：コスト差、正解が段1の候補にあるか、段1の 1 位が正解か、段2の 1 位が正解か、margin、段1の ms、段2の ms。
use std::time::Instant;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_scorer::{Config, PromptFormat, Request, Scorer};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, dict, model, temperature, examples] = &args[..] else {
        eprintln!("usage: stage2_dev <aiokey.dic> <stage2.gguf> <temperature> <examples.tsv>");
        std::process::exit(2);
    };
    let dict = Dictionary::open(dict.as_ref()).unwrap();
    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let mut scorer = Scorer::open(
        model.as_ref(),
        &Config {
            format: PromptFormat::Aiokey,
            threads,
            temperature: temperature.parse().expect("temperature must be a number"),
            // 段2で採点する候補の上限（段1の上位から。既定 32）
            max_candidates: std::env::var("STAGE2_K")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(32),
        },
    )
    .unwrap();
    println!("gap\tgold_in\tstage1_ok\tstage2_ok\tmargin\tstage1_ms\tstage2_ms");
    for line in std::fs::read_to_string(examples).unwrap().lines() {
        let mut fields = line.split('\t');
        let (Some(input), Some(context), Some(target)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let started = Instant::now();
        let candidates = convert(&dict, input, &Options { k: 32 });
        let stage1_ms = started.elapsed().as_secs_f64() * 1000.0;
        if candidates.len() < 2 {
            continue;
        }
        let gap = candidates[1].cost.saturating_sub(candidates[0].cost);
        let gold_in = candidates.iter().any(|c| c.surface == target);
        let started = Instant::now();
        let scores = scorer
            .score(&Request {
                context,
                input,
                candidates: candidates
                    .iter()
                    .map(|c| (c.surface.as_str(), c.reading.as_str()))
                    .collect(),
            })
            .unwrap();
        let stage2_ms = started.elapsed().as_secs_f64() * 1000.0;
        let best = (0..scores.probs.len())
            .max_by(|&a, &b| scores.probs[a].total_cmp(&scores.probs[b]))
            .unwrap();
        println!(
            "{gap}\t{}\t{}\t{}\t{:.4}\t{stage1_ms:.2}\t{stage2_ms:.2}",
            gold_in as u8,
            (candidates[0].surface == target) as u8,
            (candidates[best].surface == target) as u8,
            scores.margin,
        );
    }
}
