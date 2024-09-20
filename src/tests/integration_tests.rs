#[cfg(test)]
mod integration_tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::io::{AsyncWriteExt, AsyncReadExt};

    #[tokio::test]
    async fn test_client_server_interaction() {
        // Start server (use port 8080 for TCP in the server implementation)
        tokio::spawn(async {
            main().await.unwrap();
        });

        // Give server some time to start
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Connect client to the server
        let mut stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();

        // Test register command
        let register_cmd = "REGISTER|newuser|newpass";
        stream.write_all(register_cmd.as_bytes()).await.unwrap();

        let mut buffer = [0; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        let response = std::str::from_utf8(&buffer[..n]).unwrap();
        assert_eq!(response, "REGISTER|SUCCESS");

        // Test authentication
        let auth_cmd = "AUTH|newuser|newpass";
        stream.write_all(auth_cmd.as_bytes()).await.unwrap();

        let n = stream.read(&mut buffer).await.unwrap();
        let response = std::str::from_utf8(&buffer[..n]).unwrap();
        assert!(response.starts_with("AUTH|SUCCESS"));
    }
}