use std::sync::Mutex;

use aiokey_ipc::proto::{
    aiokey_service_server::AiokeyService, AppendTextRequest, AppendTextResponse, ClearTextRequest,
    ClearTextResponse, ComposingText, MoveCursorRequest, MoveCursorResponse, RemoveTextRequest,
    RemoveTextResponse, SetContextRequest, SetContextResponse, ShrinkTextRequest,
    ShrinkTextResponse, Suggestion, UpdateConfigRequest, UpdateConfigResponse,
};
use tonic::{Request, Response, Status};

/// 変換エンジンを入れるまでのスタブ。入力された文字列をそのまま 1 件の候補として返す。
/// 入力が空のときは候補を返さない（空の候補を 1 件返すと TSF 側に空の候補が出るため）。
/// 空文字の AppendText は InvalidArgument で拒否する（TSF 側の AppendText は候補 0 件を想定していないため）。
#[derive(Debug, Default)]
pub struct EchoService {
    input: Mutex<String>,
}

impl EchoService {
    fn composing_text(input: &str) -> Option<ComposingText> {
        let suggestions = if input.is_empty() {
            vec![]
        } else {
            vec![Suggestion {
                text: input.to_string(),
                subtext: String::new(),
                corresponding_count: input.chars().count() as i32,
            }]
        };
        Some(ComposingText {
            hiragana: input.to_string(),
            suggestions,
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
        if text.is_empty() {
            return Err(Status::invalid_argument("text_to_append is empty"));
        }
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
