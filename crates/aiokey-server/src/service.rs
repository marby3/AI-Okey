use std::sync::Mutex;

use aiokey_ipc::proto::{
    aiokey_service_server::AiokeyService, AppendTextRequest, AppendTextResponse, ClearTextRequest,
    ClearTextResponse, ComposingText, MoveCursorRequest, MoveCursorResponse, RemoveTextRequest,
    RemoveTextResponse, SetContextRequest, SetContextResponse, ShrinkTextRequest,
    ShrinkTextResponse, Suggestion, UpdateConfigRequest, UpdateConfigResponse,
};
use tonic::{Request, Response, Status};

/// 変換エンジンを入れるまでのスタブ。入力された文字列をそのまま 1 件の候補として返す。
#[derive(Debug, Default)]
pub struct EchoService {
    input: Mutex<String>,
}

impl EchoService {
    fn composing_text(input: &str) -> Option<ComposingText> {
        Some(ComposingText {
            hiragana: input.to_string(),
            suggestions: vec![Suggestion {
                text: input.to_string(),
                subtext: String::new(),
                corresponding_count: input.chars().count() as i32,
            }],
        })
    }

    fn edit(&self, f: impl FnOnce(&mut String)) -> Option<ComposingText> {
        let mut input = self.input.lock().unwrap();
        f(&mut input);
        Self::composing_text(&input)
    }
}

#[tonic::async_trait]
impl AiokeyService for EchoService {
    async fn append_text(
        &self,
        request: Request<AppendTextRequest>,
    ) -> Result<Response<AppendTextResponse>, Status> {
        let text = request.into_inner().text_to_append;
        let composing_text = self.edit(|input| input.push_str(&text));
        Ok(Response::new(AppendTextResponse { composing_text }))
    }

    async fn remove_text(
        &self,
        _: Request<RemoveTextRequest>,
    ) -> Result<Response<RemoveTextResponse>, Status> {
        let composing_text = self.edit(|input| {
            input.pop();
        });
        Ok(Response::new(RemoveTextResponse { composing_text }))
    }

    async fn shrink_text(
        &self,
        _: Request<ShrinkTextRequest>,
    ) -> Result<Response<ShrinkTextResponse>, Status> {
        let composing_text = self.edit(|_| {});
        Ok(Response::new(ShrinkTextResponse { composing_text }))
    }

    async fn move_cursor(
        &self,
        _: Request<MoveCursorRequest>,
    ) -> Result<Response<MoveCursorResponse>, Status> {
        let composing_text = self.edit(|_| {});
        Ok(Response::new(MoveCursorResponse { composing_text }))
    }

    async fn clear_text(
        &self,
        _: Request<ClearTextRequest>,
    ) -> Result<Response<ClearTextResponse>, Status> {
        self.edit(String::clear);
        Ok(Response::new(ClearTextResponse {}))
    }

    async fn set_context(
        &self,
        _: Request<SetContextRequest>,
    ) -> Result<Response<SetContextResponse>, Status> {
        Ok(Response::new(SetContextResponse {}))
    }

    async fn update_config(
        &self,
        _: Request<UpdateConfigRequest>,
    ) -> Result<Response<UpdateConfigResponse>, Status> {
        Ok(Response::new(UpdateConfigResponse {}))
    }
}
