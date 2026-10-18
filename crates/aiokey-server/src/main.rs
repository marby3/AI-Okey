use aiokey_ipc::proto::aiokey_service_server::AiokeyServiceServer;
use aiokey_server::service::EchoService;
use tonic::transport::Server;
use tonic_reflection::server::Builder as ReflectionBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reflection = ReflectionBuilder::configure()
        .register_encoded_file_descriptor_set(aiokey_ipc::proto::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let server = Server::builder()
        .add_service(AiokeyServiceServer::new(EchoService::default()))
        .add_service(reflection);

    #[cfg(windows)]
    {
        println!(r"aiokey-server listening on \\.\pipe\aiokey_server");
        server
            .serve_with_incoming(aiokey_server::TonicNamedPipeServer::new("aiokey_server"))
            .await?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        // 他のユーザーから接続されないよう、ユーザー専用の XDG_RUNTIME_DIR にだけ置く
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .map_err(|_| "XDG_RUNTIME_DIR is not set; refusing to create the socket elsewhere")?;
        let path = std::path::Path::new(&dir).join("aiokey_server.sock");
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        println!("aiokey-server listening on {}", path.display());
        server
            .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(listener))
            .await?;
    }

    Ok(())
}
