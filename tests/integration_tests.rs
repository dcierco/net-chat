use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tokio::sync::oneshot;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn test_client_server_interaction() {
    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

    let shutdown_future = async {
        let _ = shutdown_receiver.await;
    };

    // Start the server in a separate task
    let server_handle = tokio::spawn(async {
        net_chat::server::run_server(shutdown_future).await.expect("Server failed to run");
    });

    // Give the server some time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    let server_addr = "127.0.0.1:8080";
    let mut client = TestClient::new(server_addr).await.expect("Failed to create test client");

    // Test registration
    client.send_and_verify("alice", "REGISTER|alice", "REGISTER|SUCCESS").await
        .expect("Alice registration failed");

    client.send_and_verify("bob", "REGISTER|bob", "REGISTER|SUCCESS").await
        .expect("Bob registration failed");

    // Test sending a message
    client.send_command_as("alice", "MESSAGE|alice|bob|hello|TCP").await.expect("Failed to send MESSAGE command");

    // Add a small delay to allow for message processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Wait for Alice's confirmation
    let alice_response = client.receive_tcp_as("alice").await.expect("Failed to receive Alice's confirmation");
    assert!(alice_response.contains("MESSAGE|SUCCESS"), "Message sending failed: {}", alice_response);

    // Wait for Bob's message
    let bob_response = client.receive_tcp_as("bob").await.expect("Failed to receive Bob's message");
    assert!(bob_response.contains("MESSAGE|alice|hello"), "Unexpected message content for Bob: {}", bob_response);

    // Test listing users
    client.send_and_verify("alice", "LIST_USERS|alice", "LIST_USERS|SUCCESS").await
        .expect("User listing failed");

    // Test disconnecting
    client.send_and_verify("alice", "DISCONNECT|alice", "DISCONNECT|SUCCESS").await
        .expect("Disconnection failed");

    // Shutdown the server
    shutdown_sender.send(()).expect("Failed to send shutdown signal");

    // Wait for the server to shut down
    server_handle.await.expect("Server task panicked");
}

struct TestClient {
    alice_stream: Arc<Mutex<TcpStream>>,
    bob_stream: Arc<Mutex<TcpStream>>,
}

impl TestClient {
    async fn new(server_addr: &str) -> Result<Self, Box<dyn Error>> {
        Ok(TestClient {
            alice_stream: Arc::new(Mutex::new(TcpStream::connect(server_addr).await?)),
            bob_stream: Arc::new(Mutex::new(TcpStream::connect(server_addr).await?)),
        })
    }

    async fn send_command_as(&mut self, user: &str, command: &str) -> std::io::Result<()> {
        let stream = if user == "alice" { &self.alice_stream } else { &self.bob_stream };
        let mut locked_stream = stream.lock().await;
        println!("Sending command as {}: {}", user, command);
        locked_stream.write_all(command.as_bytes()).await?;
        locked_stream.write_all(b"\n").await?;
        locked_stream.flush().await?;
        Ok(())
    }

    async fn receive_tcp_as(&mut self, user: &str) -> std::io::Result<String> {
        let stream = if user == "alice" { &self.alice_stream } else { &self.bob_stream };
        let mut locked_stream = stream.lock().await;
        let mut buffer = [0; 1024];
        println!("Waiting to receive data as {}", user);
        match timeout(Duration::from_secs(10), locked_stream.read(&mut buffer)).await {
            Ok(Ok(n)) if n > 0 => {
                let response = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
                println!("Received data as {}: {}", user, response);
                Ok(response)
            },
            Ok(Ok(_)) => Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "No data available")),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Receive operation timed out")),
        }
    }

    async fn send_and_verify(&mut self, user: &str, command: &str, expected_response: &str) -> Result<(), Box<dyn Error>> {
        self.send_command_as(user, command).await?;
        let response = timeout(Duration::from_secs(5), self.receive_tcp_as(user)).await??;
        if !response.contains(expected_response) {
            return Err(format!("Unexpected response for {}. Expected: {}, Got: {}", user, expected_response, response).into());
        }
        Ok(())
    }
}