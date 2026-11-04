//! 段1の並べ替えの遅延とメモリを、サーバーと同じコードで測る（#58。docs/eval/stage1-ngram.md）。
//!
//!     cargo run --release -p aiokey-server --example stage1_bench -- --dict build/aiokey.dic --inputs inputs.txt \
//!         [--ngram aiokey-ngram.bin --pool 128 --cost-per-nat 250 --max-entries 80] [--model aiokey-stage2-q8_0.gguf --temperature 2.87]
//!
//! `--inputs` は 1 行に 1 入力。結果は 1 行の JSON：段1だけの時間、1 回の変換（段1＋段2）の時間、メモリ（unix の VmRSS / VmHWM）。
//! ほかの計測を同時に流さずに測る。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use aiokey_engine::convert::{convert_reranked, Options, Rerank};
use aiokey_engine::dict::Dictionary;
use aiokey_engine::ngram::Ngram;
use aiokey_ipc::proto::{
    aiokey_service_server::AiokeyService, AppendTextRequest, ClearTextRequest, ConvertRequest,
};
use aiokey_server::config::Config;
use aiokey_server::service::{model_loader, ConversionService};
use tonic::Request;

/// /proc/self/status の値（MB）。
fn memory(name: &str) -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

#[tokio::main]
async fn main() {
    let mut config = Config::default();
    let (mut dict, mut inputs, mut table) = (None, None, None);
    let mut args = std::env::args().skip(1);
    while let (Some(name), Some(value)) = (args.next(), args.next()) {
        match name.as_str() {
            "--dict" => dict = Some(PathBuf::from(value)),
            "--inputs" => inputs = Some(PathBuf::from(value)),
            "--ngram" => table = Some(PathBuf::from(value)),
            "--pool" => config.ngram.pool = value.parse().unwrap(),
            "--cost-per-nat" => config.ngram.cost_per_nat = value.parse().unwrap(),
            "--max-entries" => config.ngram.max_entries_per_span = value.parse().unwrap(),
            "--model" => config.stage2.model = Some(PathBuf::from(value)),
            "--temperature" => config.stage2.temperature = Some(value.parse().unwrap()),
            "--skip-cost-gap" => config.stage2.skip_cost_gap = value.parse().unwrap(),
            _ => panic!("unknown option {name}"),
        }
    }
    let dict = Arc::new(Dictionary::open(&dict.expect("--dict")).unwrap());
    let ngram = table.map(|p| Arc::new(Ngram::open(&p).unwrap()));
    let text = std::fs::read_to_string(inputs.expect("--inputs")).unwrap();
    let inputs: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let settings = config.ngram.clone();
    let k = config.k;
    let loader = model_loader(&config.stage2);
    let service = ConversionService::with_ngram(dict.clone(), ngram.clone(), config, loader);
    let start_rss = memory("VmRSS");

    let rerank = ngram.as_deref().map(|ngram| Rerank {
        ngram,
        pool: settings.pool,
        cost_per_nat: settings.cost_per_nat,
        max_entries_per_span: settings.max_entries_per_span,
    });
    let (mut stage1, mut total) = (Vec::new(), Vec::new());
    for input in &inputs {
        let t = Instant::now();
        std::hint::black_box(convert_reranked(
            &dict,
            input,
            &Options { k },
            rerank.as_ref(),
        ));
        stage1.push(t.elapsed().as_secs_f64() * 1000.0);

        service
            .clear_text(Request::new(ClearTextRequest {}))
            .await
            .unwrap();
        let t = Instant::now();
        service
            .append_text(Request::new(AppendTextRequest {
                text_to_append: input.to_string(),
            }))
            .await
            .unwrap();
        service
            .convert(Request::new(ConvertRequest {}))
            .await
            .unwrap();
        total.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    stage1.sort_by(f64::total_cmp);
    total.sort_by(f64::total_cmp);
    println!(
        "{{\"inputs\":{},\"stage1_p50\":{:.1},\"stage1_p95\":{:.1},\"total_p50\":{:.0},\"total_p95\":{:.0},\"rss_start_mb\":{:.0},\"rss_end_mb\":{:.0},\"peak_mb\":{:.0}}}",
        inputs.len(),
        percentile(&stage1, 0.5),
        percentile(&stage1, 0.95),
        percentile(&total, 0.5),
        percentile(&total, 0.95),
        start_rss,
        memory("VmRSS"),
        memory("VmHWM"),
    );
}
