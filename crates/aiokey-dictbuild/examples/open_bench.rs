//! 辞書を開く時間と、1 語を引く時間を測る。
//!
//!     cargo run --release -p aiokey-dictbuild --example open_bench -- target/aiokey.dic

fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    for i in 0..3 {
        let t = std::time::Instant::now();
        let d = aiokey_engine::dict::Dictionary::open(&path).unwrap();
        let t_open = t.elapsed();
        let t = std::time::Instant::now();
        let n = d.lookup("htst").len();
        println!(
            "open#{i} {:?}  lookup(htst) {} entries in {:?}",
            t_open,
            n,
            t.elapsed()
        );
    }
}
