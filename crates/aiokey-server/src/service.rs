//! 変換サービス（OS に依存しない）。
use std::sync::Arc;

use aiokey_engine::dict::Dictionary;
use aiokey_ipc::proto::{
    aiokey_service_server::AiokeyService, AppendTextRequest, AppendTextResponse, ClearTextRequest,
    ClearTextResponse, ConvertRequest, ConvertResponse, GetComposingTextRequest,
    GetComposingTextResponse, MoveCursorRequest, MoveCursorResponse, RemoveTextRequest,
    RemoveTextResponse, SetContextRequest, SetContextResponse, ShrinkTextRequest,
    ShrinkTextResponse, UpdateConfigRequest, UpdateConfigResponse,
};
use tonic::{Request, Response, Status};

use crate::config::Config;

pub trait Reranker: Send {
    fn score(
        &mut self,
        context: &str,
        input: &str,
        candidates: &[(String, String)],
    ) -> Result<Vec<f32>, String>;
}

pub type Loader = Arc<dyn Fn() -> Result<Box<dyn Reranker>, String> + Send + Sync>;

pub struct ConversionService {}

impl ConversionService {
    pub fn new(_dictionary: Arc<Dictionary>, _config: Config, _loader: Option<Loader>) -> Self {
        todo!()
    }

    pub fn stage2_loaded(&self) -> bool {
        todo!()
    }
}

#[tonic::async_trait]
impl AiokeyService for ConversionService {
    async fn append_text(
        &self,
        _: Request<AppendTextRequest>,
    ) -> Result<Response<AppendTextResponse>, Status> {
        todo!()
    }
    async fn remove_text(
        &self,
        _: Request<RemoveTextRequest>,
    ) -> Result<Response<RemoveTextResponse>, Status> {
        todo!()
    }
    async fn shrink_text(
        &self,
        _: Request<ShrinkTextRequest>,
    ) -> Result<Response<ShrinkTextResponse>, Status> {
        todo!()
    }
    async fn move_cursor(
        &self,
        _: Request<MoveCursorRequest>,
    ) -> Result<Response<MoveCursorResponse>, Status> {
        todo!()
    }
    async fn clear_text(
        &self,
        _: Request<ClearTextRequest>,
    ) -> Result<Response<ClearTextResponse>, Status> {
        todo!()
    }
    async fn set_context(
        &self,
        _: Request<SetContextRequest>,
    ) -> Result<Response<SetContextResponse>, Status> {
        todo!()
    }
    async fn update_config(
        &self,
        _: Request<UpdateConfigRequest>,
    ) -> Result<Response<UpdateConfigResponse>, Status> {
        todo!()
    }
    async fn convert(
        &self,
        _: Request<ConvertRequest>,
    ) -> Result<Response<ConvertResponse>, Status> {
        todo!()
    }
    async fn get_composing_text(
        &self,
        _: Request<GetComposingTextRequest>,
    ) -> Result<Response<GetComposingTextResponse>, Status> {
        todo!()
    }
}
