use aiokey_ipc::proto::{
    aiokey_service_client::AiokeyServiceClient, window_service_client::WindowServiceClient,
};
use anyhow::Result;
use hyper_util::rt::TokioIo;
use std::{sync::Arc, time::Duration};
use tokio::{net::windows::named_pipe::ClientOptions, time};
use tonic::transport::Endpoint;
use tower::service_fn;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

// connect to kkc server
#[derive(Debug, Clone)]
pub struct IPCService {
    // kkc server client
    engine_client: AiokeyServiceClient<tonic::transport::channel::Channel>,
    // candidate window server client
    window_client: WindowServiceClient<tonic::transport::channel::Channel>,
    runtime: Arc<tokio::runtime::Runtime>,
}

#[derive(Debug, Clone, Default)]
pub struct Candidates {
    pub texts: Vec<String>,
    pub sub_texts: Vec<String>,
    pub hiragana: String,
    pub corresponding_count: Vec<i32>,
    /// 入力の番号（段2の結果がどの入力のものかを見分ける）
    pub revision: u64,
    /// 段2が済んだ（または呼ばないと決めた）か
    pub settled: bool,
    /// 段2で並べ替え済みか
    pub reranked: bool,
    /// 段2の確信が低い（margin が閾値未満）
    pub uncertain: bool,
}

impl TryFrom<Option<aiokey_ipc::proto::ComposingText>> for Candidates {
    type Error = anyhow::Error;

    fn try_from(composing_text: Option<aiokey_ipc::proto::ComposingText>) -> anyhow::Result<Self> {
        let Some(c) = composing_text else {
            anyhow::bail!("composing_text is None");
        };
        Ok(Candidates {
            texts: c.suggestions.iter().map(|s| s.text.clone()).collect(),
            sub_texts: c.suggestions.iter().map(|s| s.subtext.clone()).collect(),
            corresponding_count: c
                .suggestions
                .iter()
                .map(|s| s.corresponding_count)
                .collect(),
            hiragana: c.hiragana,
            revision: c.revision,
            settled: c.settled,
            reranked: c.reranked,
            uncertain: c.uncertain,
        })
    }
}

impl IPCService {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;

        let server_channel = runtime.block_on(
            Endpoint::try_from("http://[::]:50051")?.connect_with_connector(service_fn(
                |_| async {
                    let client = loop {
                        match ClientOptions::new().open(r"\\.\pipe\aiokey_server") {
                            Ok(client) => break client,
                            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => (),
                            Err(e) => return Err(e),
                        }

                        time::sleep(Duration::from_millis(50)).await;
                    };

                    Ok::<_, std::io::Error>(TokioIo::new(client))
                },
            )),
        )?;

        let ui_channel = runtime.block_on(
            Endpoint::try_from("http://[::]:50052")?.connect_with_connector(service_fn(
                |_| async {
                    let client = loop {
                        match ClientOptions::new().open(r"\\.\pipe\aiokey_ui") {
                            Ok(client) => break client,
                            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => (),
                            Err(e) => return Err(e),
                        }

                        time::sleep(Duration::from_millis(50)).await;
                    };

                    Ok::<_, std::io::Error>(TokioIo::new(client))
                },
            )),
        )?;

        let engine_client = AiokeyServiceClient::new(server_channel);
        let window_client = WindowServiceClient::new(ui_channel);
        tracing::debug!("Connected to server: {:?}", engine_client);

        Ok(Self {
            engine_client,
            window_client,
            runtime: Arc::new(runtime),
        })
    }
}

// implement methods to interact with kkc server
impl IPCService {
    #[tracing::instrument]
    pub fn append_text(&mut self, text: String) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(aiokey_ipc::proto::AppendTextRequest {
            text_to_append: text,
        });

        let response = self
            .runtime
            .clone()
            .block_on(self.engine_client.append_text(request))?;
        Candidates::try_from(response.into_inner().composing_text)
    }

    #[tracing::instrument]
    pub fn remove_text(&mut self) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(aiokey_ipc::proto::RemoveTextRequest {});
        let response = self
            .runtime
            .clone()
            .block_on(self.engine_client.remove_text(request))?;
        Candidates::try_from(response.into_inner().composing_text)
    }

    #[tracing::instrument]
    pub fn clear_text(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::ClearTextRequest {});
        let _response = self
            .runtime
            .clone()
            .block_on(self.engine_client.clear_text(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn shrink_text(&mut self, offset: i32) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(aiokey_ipc::proto::ShrinkTextRequest { offset });
        let response = self
            .runtime
            .clone()
            .block_on(self.engine_client.shrink_text(request))?;
        Candidates::try_from(response.into_inner().composing_text)
    }

    /// 変換キー：今の入力を段2で並べ替えた候補（段1の確信度が高ければ段1のまま）。
    #[tracing::instrument]
    pub fn convert(&mut self) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(aiokey_ipc::proto::ConvertRequest {});
        let response = self
            .runtime
            .clone()
            .block_on(self.engine_client.convert(request))?;
        Candidates::try_from(response.into_inner().composing_text)
    }

    /// 今の入力の最新の候補（段2の結果があればそれ）。`wait_ms` まで段2を待つ。
    pub fn get_composing_text(&mut self, wait_ms: u32) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(aiokey_ipc::proto::GetComposingTextRequest { wait_ms });
        let response = self
            .runtime
            .clone()
            .block_on(self.engine_client.get_composing_text(request))?;
        Candidates::try_from(response.into_inner().composing_text)
    }

    pub fn set_context(&mut self, context: String) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::SetContextRequest { context });
        let _response = self
            .runtime
            .clone()
            .block_on(self.engine_client.set_context(request))?;

        Ok(())
    }
}

// implement methods to interact with candidate window server
impl IPCService {
    #[tracing::instrument]
    pub fn show_window(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::EmptyResponse {});
        self.runtime
            .clone()
            .block_on(self.window_client.show_window(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn hide_window(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::EmptyResponse {});
        self.runtime
            .clone()
            .block_on(self.window_client.hide_window(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_window_position(
        &mut self,
        top: i32,
        left: i32,
        bottom: i32,
        right: i32,
    ) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::SetPositionRequest {
            position: Some(aiokey_ipc::proto::WindowPosition {
                top,
                left,
                bottom,
                right,
            }),
        });
        self.runtime
            .clone()
            .block_on(self.window_client.set_window_position(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_candidates(&mut self, candidates: Vec<String>) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::SetCandidateRequest { candidates });
        self.runtime
            .clone()
            .block_on(self.window_client.set_candidate(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_selection(&mut self, index: i32) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::SetSelectionRequest { index });
        self.runtime
            .clone()
            .block_on(self.window_client.set_selection(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_input_mode(&mut self, mode: &str) -> anyhow::Result<()> {
        let request = tonic::Request::new(aiokey_ipc::proto::SetInputModeRequest {
            mode: mode.to_string(),
        });
        self.runtime
            .clone()
            .block_on(self.window_client.set_input_mode(request))?;

        Ok(())
    }
}
