//! AI-O Key の候補ウィンドウとモード表示（Windows 専用）。

#[cfg(windows)]
mod app;

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        app::run()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("aiokey-ui runs only on Windows")
    }
}
