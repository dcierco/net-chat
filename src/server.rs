use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{RwLock, Mutex};
use tokio::time::{timeout, Duration};

use log::{info, warn, error};
use tokio::sync::oneshot;

// Define AsyncStream trait
trait AsyncStream: AsyncRead + AsyncWrite + Send + Sync + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Sync + Unpin> AsyncStream for T {}

#[derive(Debug, Clone)]
enum Command {
    Register(String),
    SendMessage(String, String, String, Transport),
    ListUsers(String),
    Disconnect(String),
}

#[derive(Debug, Clone)]
enum Transport {
    Tcp,
    Udp,
}

impl Command {
    fn from_str(command: &str) -> Option<Self> {
        let command = command.trim();
        let parts: Vec<&str> = command.split('|').collect();
        match parts[0] {
            "REGISTER" if parts.len() == 2 => Some(Command::Register(parts[1].to_string())),
            "MESSAGE" if parts.len() == 5 => Some(Command::SendMessage(
                parts[1].to_string(),
                parts[2].to_string(),
                parts[3].to_string(),
                Transport::from_str(parts[4])?,
            )),
           "LIST_USERS" if parts.len() == 2 => Some(Command::ListUsers(parts[1].to_string())),
            "DISCONNECT" if parts.len() == 2 => Some(Command::Disconnect(parts[1].to_string())),
            _ => None,
        }
    }
}

impl Transport {
    fn from_str(transport: &str) -> Option<Self> {
        match transport {
            "TCP" => Some(Transport::Tcp),
            "UDP" => Some(Transport::Udp),
            _ => None,
        }
    }
}

struct User {
    tcp_stream: Option<Arc<Mutex<Box<dyn AsyncStream + Send>>>>,
    udp_addr: Option<SocketAddr>,
}

struct Server {
    online_users: Arc<Mutex<HashMap<String, User>>>,
    udp_socket: Arc<UdpSocket>,
}

impl Server {
    async fn register_user(&self, username: &str, tcp_stream: Option<Arc<Mutex<Box<dyn AsyncStream + Send>>>>, udp_addr: Option<SocketAddr>) -> Result<(), String> {
        let mut online_users = self.online_users.lock().await;
        if online_users.contains_key(username) {
            Err("Username already taken".into())
        } else {
            online_users.insert(username.to_string(), User {
                tcp_stream,
                udp_addr,
            });
            info!("User registered: {}", username);
            Ok(())
        }
    }

    async fn list_users(&self, username: &str) -> String {
        let online_users = self.online_users.lock().await;
        if !online_users.contains_key(username) {
            return "LIST_USERS|FAILURE|User not registered".into();
        }
        let users = online_users.keys().cloned().collect::<Vec<_>>().join(",");
        info!("Listed users for: {}", username);
        format!("LIST_USERS|SUCCESS|{}", users)
    }

    async fn disconnect(&self, username: &str) -> String {
        let mut online_users = self.online_users.lock().await;
        if online_users.remove(username).is_some() {
            info!("User disconnected: {}", username);
            "DISCONNECT|SUCCESS".into()
        } else {
            "DISCONNECT|FAILURE|User not registered".into()
        }
    }

    async fn send_message(&self, from: &str, to: &str, content: &str, transport: &Transport) -> Result<(), String> {
        println!("Entering send_message: from={}, to={}, content={:?}, transport={:?}", from, to, content, transport);

        let (sender_stream, receiver_stream, receiver_udp_addr) = {
            let online_users = self.online_users.lock().await;
            println!("Acquired online_users lock");

            let sender = online_users.get(from).ok_or_else(|| format!("Sender {} not registered", from))?;
            let receiver = online_users.get(to).ok_or_else(|| format!("Recipient {} not online", to))?;

            (sender.tcp_stream.clone(), receiver.tcp_stream.clone(), receiver.udp_addr)
        };
        println!("Released online_users lock");

        let message = format!("MESSAGE|{}|{}\n", from, content);

        match transport {
            Transport::Tcp => {
                if let Some(tcp_stream) = receiver_stream {
                    println!("Attempting to acquire receiver's socket lock");
                    let mut socket_lock = tcp_stream.lock().await;
                    println!("Acquired receiver's socket lock");

                    println!("Writing message to receiver");
                    socket_lock.write_all(message.as_bytes()).await.map_err(|e| format!("Failed to send TCP message: {}", e))?;
                    socket_lock.flush().await.map_err(|e| format!("Failed to flush receiver's socket: {}", e))?;
                    println!("Message sent to {}", to);
                    drop(socket_lock);
                    println!("Released receiver's socket lock");

                    // Send confirmation to sender
                    if let Some(sender_stream) = sender_stream {
                        println!("Attempting to send confirmation to sender");
                        let mut sender_lock = sender_stream.lock().await;
                        println!("Acquired sender's socket lock");
                        sender_lock.write_all(b"MESSAGE|SUCCESS\n").await.map_err(|e| format!("Failed to send confirmation: {}", e))?;
                        sender_lock.flush().await.map_err(|e| format!("Failed to flush sender's socket: {}", e))?;
                        println!("Confirmation sent to {}", from);
                        drop(sender_lock);
                        println!("Released sender's socket lock");
                    } else {
                        println!("Sender does not have a TCP stream");
                        return Err(format!("Sender {} does not have a TCP stream", from));
                    }

                    println!("Message successfully sent from {} to {} over TCP", from, to);
                    Ok(())
                } else {
                    Err(format!("Recipient {} not connected via TCP", to))
                }
            }
            Transport::Udp => {
                if let Some(udp_addr) = receiver_udp_addr {
                    println!("Sending message via UDP");
                    self.udp_socket.send_to(message.as_bytes(), udp_addr).await
                        .map_err(|e| format!("Failed to send UDP message: {}", e))?;
                    println!("Message sent from {} to {} over UDP", from, to);
                    Ok(())
                } else {
                    Err(format!("Recipient {} not connected via UDP", to))
                }
            }
        }
    }


    async fn handle_command(&self, command: Command, tcp_stream: Option<Arc<Mutex<Box<dyn AsyncStream + Send>>>>, udp_addr: Option<SocketAddr>) -> (String, Option<String>) {
        match command {
            Command::Register(username) => {
                info!("Handling REGISTER command for user: {}", username);
                match self.register_user(&username, tcp_stream, udp_addr).await {
                    Ok(_) => (format!("REGISTER|SUCCESS"), Some(username)),
                    Err(e) => (format!("REGISTER|FAILURE|{}", e), None),
                }
            }
            Command::SendMessage(from, to, content, transport) => {
                info!("Handling SendMessage: from={}, to={}, content={:?}, transport={:?}", from, to, content, transport);
                match self.send_message(&from, &to, &content, &transport).await {
                    Ok(_) => {
                        info!("Message sent successfully");
                        ("MESSAGE|SUCCESS".into(), None)
                    },
                    Err(e) => {
                        error!("Failed to send message: {}", e);
                        (format!("MESSAGE|FAILURE|{}", e), None)
                    },
                }
            }
            Command::ListUsers(username) => {
                info!("Handling LIST USERS command for user: {}", username);
                (self.list_users(&username).await, None)
            }
            Command::Disconnect(username) => {
                info!("Handling DISCONNECT command for user: {}", username);
                (self.disconnect(&username).await, None)
            }
        }
    }

    async fn handle_tcp_connection(self: Arc<Self>, socket: TcpStream) {
        let socket: Arc<Mutex<Box<dyn AsyncStream + Send>>> = Arc::new(Mutex::new(Box::new(socket)));
        let mut buffer = [0; 2048];
        let mut username = String::new();

        loop {
            let mut locked_socket = socket.lock().await;
            match locked_socket.read(&mut buffer).await {
                Ok(0) => {
                    info!("Client disconnected");
                    break;
                }
                Ok(n) => {
                    let command_str = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
                    info!("Received command: {}", command_str);
                    if command_str.is_empty() {
                        continue;
                    }
                    if let Some(command) = Command::from_str(&command_str) {
                        let (response, new_username) = self.handle_command(command, Some(Arc::clone(&socket)), None).await;
                        info!("Sending response: {}", response);
                        if let Err(e) = locked_socket.write_all(response.as_bytes()).await {
                            error!("Failed to write response: {}", e);
                            continue;
                        }
                        if let Err(e) = locked_socket.write_all(b"\n").await {
                            error!("Failed to write newline: {}", e);
                            continue;
                        }
                        if let Err(e) = locked_socket.flush().await {
                            error!("Failed to flush socket: {}", e);
                            continue;
                        }
                        info!("Response sent successfully");

                        // Update username if provided
                        if let Some(new_name) = new_username {
                            username = new_name;
                            info!("Updated username to: {}", username);
                        }
                    } else {
                        warn!("Unknown command: {}", command_str);
                        if let Err(e) = locked_socket.write_all(b"ERROR|Unknown command\n").await {
                            error!("Failed to write error response: {}", e);
                            continue;
                        }
                        if let Err(e) = locked_socket.flush().await {
                            error!("Failed to flush socket after error: {}", e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    error!("TCP connection error: {:?}", e);
                    break;
                },
            }
        }
        info!("TCP connection closed for user: {}", username);
        if !username.is_empty() {
            let _ = self.disconnect(&username).await;
        }
    }

    async fn handle_udp_socket(self: Arc<Self>) {
        let mut buffer = [0; 2048];
        loop {
            match self.udp_socket.recv_from(&mut buffer).await {
                Ok((n, addr)) => {
                    let command_str = String::from_utf8_lossy(&buffer[..n]);
                    if let Some(command) = Command::from_str(&command_str) {
                        let (response, _) = self.handle_command(command, None, Some(addr)).await;
                        if self.udp_socket.send_to(response.as_bytes(), addr).await.is_err() {
                            error!("Error sending UDP response");
                        }
                    } else {
                        if self.udp_socket.send_to(b"ERROR|Unknown command", addr).await.is_err() {
                            error!("Error sending UDP error response");
                        }
                    }
                },
                Err(e) => {
                    warn!("UDP receive error: {:?}", e);
                },
            }
        }
    }
}

pub async fn run_server(shutdown: impl Future<Output = ()> + Send + 'static) -> io::Result<()> {    env_logger::init();

    let online_users = Arc::new(Mutex::new(HashMap::new()));
    let udp_socket = Arc::new(UdpSocket::bind("127.0.0.1:8081").await?);

    let server = Arc::new(Server {
        online_users,
        udp_socket: udp_socket.clone(),
    });

    let tcp_listener = TcpListener::bind("127.0.0.1:8080").await?;

    let tcp_server = server.clone();
    let tcp_handle = tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = tcp_listener.accept().await {
                let tcp_server = tcp_server.clone();
                tokio::spawn(async move {
                    tcp_server.handle_tcp_connection(socket).await;
                });
            }
        }
    });
    info!("TCP server running on 127.0.0.1:8080");

    let udp_server = server.clone();
    let udp_handle = tokio::spawn(async move {
        udp_server.handle_udp_socket().await;
    });
    info!("UDP server running on 127.0.0.1:8081");

    // Wait for both TCP and UDP handlers
    tokio::select! {
        _ = tcp_handle => {},
        _ = udp_handle => {},
        _ = shutdown => {
            info!("Server shutting down");
        },
    }

    Ok(())
}

#[tokio::main]
pub async fn main() -> io::Result<()> {
    let (_, shutdown_receiver) = oneshot::channel::<()>();

    // Create a never-ending future
    let shutdown_future = async {
        let _ = shutdown_receiver.await;
    };

    run_server(shutdown_future).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{self, UnboundedSender};
    use tokio::time::timeout;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    async fn create_test_server() -> Arc<Server> {
        let online_users = Arc::new(Mutex::new(HashMap::new()));
        let udp_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        Arc::new(Server {
            online_users,
            udp_socket,
        })
    }

    struct MockTcpStream {
        username: String,
        tx: UnboundedSender<String>,
    }

    impl AsyncRead for MockTcpStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            println!("MockTcpStream({}): poll_read called", self.username);
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for MockTcpStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            let msg = String::from_utf8_lossy(buf).to_string();
            println!("MockTcpStream({}): poll_write called with message: {}", self.username, msg);
            if let Err(_) = self.tx.send(msg) {
                println!("MockTcpStream({}): Failed to send message", self.username);
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Failed to send")));
            }
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            println!("MockTcpStream({}): poll_flush called", self.username);
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            println!("MockTcpStream({}): poll_shutdown called", self.username);
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn test_register_user() {
        let server = create_test_server().await;
        let result = server.register_user("alice", None, None).await;
        assert!(result.is_ok());

        let result = server.register_user("alice", None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_users() {
        let server = create_test_server().await;
        server.register_user("alice", None, None).await.unwrap();
        server.register_user("bob", None, None).await.unwrap();

        let result = server.list_users("alice").await;
        assert!(result.contains("alice"));
        assert!(result.contains("bob"));

        let result = server.list_users("charlie").await;
        assert!(result.contains("FAILURE"));
    }

    #[tokio::test]
    async fn test_disconnect() {
        let server = create_test_server().await;
        server.register_user("alice", None, None).await.unwrap();

        let result = server.disconnect("alice").await;
        assert_eq!(result, "DISCONNECT|SUCCESS");

        let result = server.disconnect("alice").await;
        assert!(result.contains("FAILURE"));
    }

    #[tokio::test]
    async fn test_send_message_tcp() {
        let server = create_test_server().await;
        let (alice_tx, mut alice_rx) = mpsc::unbounded_channel();
        let (bob_tx, mut bob_rx) = mpsc::unbounded_channel();

        let alice_stream: Arc<Mutex<Box<dyn AsyncStream + Send>>> = Arc::new(Mutex::new(Box::new(MockTcpStream {
            username: "alice".to_string(),
            tx: alice_tx,
        })));
        let bob_stream: Arc<Mutex<Box<dyn AsyncStream + Send>>> = Arc::new(Mutex::new(Box::new(MockTcpStream {
            username: "bob".to_string(),
            tx: bob_tx,
        })));

        println!("Registering users...");
        server.register_user("alice", Some(alice_stream.clone()), None).await.unwrap();
        server.register_user("bob", Some(bob_stream.clone()), None).await.unwrap();
        println!("Users registered");

        // Add a small delay to ensure everything is set up
        tokio::time::sleep(Duration::from_millis(100)).await;

        println!("Sending message...");
        let send_result = timeout(Duration::from_secs(10), server.send_message("bob", "alice", "Hello", &Transport::Tcp)).await;

        match send_result {
            Ok(result) => {
                match result {
                    Ok(_) => println!("Message sent successfully"),
                    Err(e) => {
                        eprintln!("send_message error: {}", e);
                        assert!(false, "send_message failed: {}", e);
                    }
                }
            },
            Err(_) => {
                eprintln!("send_message timed out");
                assert!(false, "send_message timed out");
            }
        }

        println!("Waiting for Alice to receive the message...");
        match timeout(Duration::from_secs(5), alice_rx.recv()).await {
            Ok(Some(received)) => {
                println!("Alice received: {}", received);
                assert_eq!(received, "MESSAGE|bob|Hello\n");
            },
            Ok(None) => {
                eprintln!("Alice's channel closed unexpectedly");
                assert!(false, "Alice's channel closed unexpectedly");
            },
            Err(_) => {
                eprintln!("Timed out waiting for Alice to receive the message");
                assert!(false, "Timed out waiting for Alice to receive the message");
            }
        }

        println!("Waiting for Bob to receive the confirmation...");
        match timeout(Duration::from_secs(5), bob_rx.recv()).await {
            Ok(Some(received)) => {
                println!("Bob received: {}", received);
                assert_eq!(received, "MESSAGE|SUCCESS\n");
            },
            Ok(None) => {
                eprintln!("Bob's channel closed unexpectedly");
                assert!(false, "Bob's channel closed unexpectedly");
            },
            Err(_) => {
                eprintln!("Timed out waiting for Bob to receive the confirmation");
                assert!(false, "Timed out waiting for Bob to receive the confirmation");
            }
        }

        println!("Test completed");
    }
}
