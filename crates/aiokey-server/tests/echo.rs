use aiokey_ipc::proto::{aiokey_service_server::AiokeyService, AppendTextRequest};
use aiokey_server::service::EchoService;
use tonic::Request;

#[tokio::test]
async fn append_text_echoes_input_as_single_candidate() {
    let service = EchoService::default();

    let response = service
        .append_text(Request::new(AppendTextRequest {
            text_to_append: "htst".to_string(),
        }))
        .await
        .unwrap()
        .into_inner();

    let composing = response.composing_text.unwrap();
    assert_eq!(composing.hiragana, "htst");
    assert_eq!(composing.suggestions.len(), 1);
    assert_eq!(composing.suggestions[0].text, "htst");
}

#[tokio::test]
async fn append_text_rejects_empty_input() {
    let service = EchoService::default();

    let status = service
        .append_text(Request::new(AppendTextRequest {
            text_to_append: String::new(),
        }))
        .await
        .unwrap_err();

    assert_eq!(status.code(), tonic::Code::InvalidArgument);
}
