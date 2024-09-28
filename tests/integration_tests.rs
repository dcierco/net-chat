use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use tokio::sync::oneshot;
use std::error::Error;
use std::thread;

use net_chat::server;

const TIMEOUT_DURATION: Duration = Duration::from_secs(5);

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
    timeout(TIMEOUT_DURATION, client.send_and_verify("alice", "REGISTER|alice", "REGISTER|SUCCESS")).await
        .expect("Timeout during Alice registration")
        .expect("Alice registration failed");

    timeout(TIMEOUT_DURATION, client.send_and_verify("bob", "REGISTER|bob", "REGISTER|SUCCESS")).await
        .expect("Timeout during Bob registration")
        .expect("Bob registration failed");

    // Test sending a message
    println!("Sending message from Alice to Bob");
    client.send_command_as("alice", "MESSAGE|alice|bob|hello|TCP").await.expect("Failed to send MESSAGE command");
    println!("Message command sent");

    // Add a small delay to allow for message processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Wait for Alice's confirmation
    println!("Waiting for Alice's confirmation...");
    let alice_response = match client.receive_tcp_as("alice").await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Failed to receive Alice's confirmation: {}", e);
            panic!("Test failed: {}", e);
        }
    };
    println!("Received response for Alice: {}", alice_response);
    assert!(alice_response.contains("MESSAGE|SUCCESS"), "Message sending failed: {}", alice_response);

    // Wait for Bob's message
    println!("Waiting for Bob's message...");
    let bob_response = match client.receive_tcp_as("bob").await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Failed to receive Bob's message: {}", e);
            panic!("Test failed: {}", e);
        }
    };
    println!("Received response for Bob: {}", bob_response);
    assert!(bob_response.contains("MESSAGE|alice|hello"), "Unexpected message content for Bob: {}", bob_response);


    // Test listing users
    timeout(TIMEOUT_DURATION, client.send_and_verify("alice", "LIST_USERS|alice", "LIST_USERS|SUCCESS")).await
        .expect("Timeout during user listing")
        .expect("User listing failed");

    // Test disconnecting
    timeout(TIMEOUT_DURATION, client.send_and_verify("alice", "DISCONNECT|alice", "DISCONNECT|SUCCESS")).await
        .expect("Timeout during disconnection")
        .expect("Disconnection failed");

    // Shutdown the server
    shutdown_sender.send(()).expect("Failed to send shutdown signal");

    // Wait for the server to shut down
    server_handle.await.expect("Server task panicked");

    println!("Test completed successfully!");
}


struct TestClient {
    alice_stream: TcpStream,
    bob_stream: TcpStream,
}

impl TestClient {
    async fn new(server_addr: &str) -> std::io::Result<Self> {
        Ok(TestClient {
            alice_stream: TcpStream::connect(server_addr).await?,
            bob_stream: TcpStream::connect(server_addr).await?,
        })
    }

    async fn send_command_as(&mut self, user: &str, command: &str) -> std::io::Result<()> {
        let stream = if user == "alice" { &mut self.alice_stream } else { &mut self.bob_stream };
        stream.write_all(command.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;
        println!("Sent command for {}: {}", user, command);
        Ok(())
    }

    async fn receive_tcp_as(&mut self, user: &str) -> std::io::Result<String> {
        let stream = if user == "alice" { &mut self.alice_stream } else { &mut self.bob_stream };
        let mut buffer = [0; 1024];
        match timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
            Ok(Ok(n)) if n > 0 => {
                let response = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
                println!("Received for {}: {:?}", user, response);
                Ok(response)
            }
            Ok(Ok(_)) => Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "No data available")),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Receive operation timed out")),
        }
    }


        async fn send_and_verify(&mut self, user: &str, command: &str, expected_response: &str) -> Result<(), Box<dyn Error>> {
                println!("Sending command for {}: {}", user, command);
                self.send_command_as(user, command).await?;

                let response = tokio::time::timeout(
                    Duration::from_secs(5),
                    self.receive_tcp_as(user)
                ).await??;

                println!("Received response for {}: {}", user, response);

                if !response.contains(expected_response) {
                    return Err(format!("Unexpected response for {}. Expected: {}, Got: {}", user, expected_response, response).into());
                }
                Ok(())
            }
        }
