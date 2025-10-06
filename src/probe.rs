use anyhow::{Result, Context};
use std::time::Duration;
use::tokio::{io::{AsyncReadExt, AsyncWriteExt}};

pub async fn health_check_server() -> Result<()> {
    let port = 8080;
    let addr = format!("0.0.0.0:{}", port);
    
    let listener = tokio::net::TcpListener::bind(&addr).await
        .context(format!("Failed to bind TCP listener to {}", addr))?;

    log::info!("[HEALTH] Health check server listening on {}", addr);

    let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK";
    let response_bytes = response.as_bytes();

    loop {
        match listener.accept().await {
            Ok((mut socket, _addr)) => {
                tokio::spawn(async move {
                    let mut buf = [0; 1024];
                    let _ = socket.read(&mut buf).await;
                    if let Err(e) = socket.write_all(response_bytes).await && e.kind() != std::io::ErrorKind::BrokenPipe {
                        log::error!("[HEALTH ERROR] Failed to write response: {}", e);
                    }
                });
            }
            Err(e) => {
                log::error!("[HEALTH ERROR] Failed to accept connection: {}", e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

