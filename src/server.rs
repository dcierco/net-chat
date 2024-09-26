use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
use log::{info, warn, error};

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
    tcp_stream: Option<Arc<Mutex<TcpStream>>>,
    udp_addr: Option<SocketAddr>,
}

struct Server {
    online_users: Arc<Mutex<HashMap<String, User>>>,
    udp_socket: Arc<UdpSocket>,
}

impl Server {
    async fn register_user(&self, username: &str, tcp_stream: Option<Arc<Mutex<TcpStream>>>, udp_addr: Option<SocketAddr>) -> Result<(), String> {
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
        let online_users = self.online_users.lock().await;
        if !online_users.contains_key(from) {
            return Err("Sender not registered".into());
        }
        let receiver = online_users.get(to).ok_or("Recipient not online")?;

        let message = format!("MESSAGE|{}|{}", from, content);

        match transport {
            Transport::Tcp => {
                if let Some(tcp_stream) = &receiver.tcp_stream {
                    let mut socket_lock = tcp_stream.lock().await;
                    socket_lock.write_all(message.as_bytes()).await
                        .map_err(|e| format!("Failed to send TCP message: {}", e))?;
                    info!("Message sent from {} to {} over TCP", from, to);
                    Ok(())
                } else {
                    Err("Recipient not connected via TCP".into())
                }
            }
            Transport::Udp => {
                if let Some(udp_addr) = receiver.udp_addr {
                    self.udp_socket.send_to(message.as_bytes(), udp_addr).await
                        .map_err(|e| format!("Failed to send UDP message: {}", e))?;
                    info!("Message sent from {} to {} over UDP", from, to);
                    Ok(())
                } else {
                    Err("Recipient not connected via UDP".into())
                }
            }
        }
    }

    async fn handle_command(&self, command: Command, tcp_stream: Option<Arc<Mutex<TcpStream>>>, udp_addr: Option<SocketAddr>) -> String {
        match command {
            Command::Register(username) => {
                match self.register_user(&username, tcp_stream, udp_addr).await {
                    Ok(_) => "REGISTER|SUCCESS".into(),
                    Err(e) => format!("REGISTER|FAILURE|{}", e),
                }
            }
            Command::SendMessage(from, to, content, transport) => {
                match self.send_message(&from, &to, &content, &transport).await {
                    Ok(_) => "MESSAGE|SUCCESS".into(),
                    Err(e) => format!("MESSAGE|FAILURE|{}", e),
                }
            }
            Command::ListUsers(username) => {
                self.list_users(&username).await
            }
            Command::Disconnect(username) => {
                self.disconnect(&username).await
            }
        }
    }

    async fn handle_tcp_connection(self: Arc<Self>, socket: TcpStream) {
        let socket = Arc::new(Mutex::new(socket));
        let mut buffer = [0; 2048];

        loop {
            let n = match socket.lock().await.read(&mut buffer).await {
                Ok(n) if n == 0 => break,
                Ok(n) => n,
                Err(e) => {
                    warn!("TCP connection error: {:?}", e);
                    break;
                },
            };

            let command_str = String::from_utf8_lossy(&buffer[..n]);
            if let Some(command) = Command::from_str(&command_str) {
                let response = self.handle_command(command, Some(socket.clone()), None).await;
                if socket.lock().await.write_all(response.as_bytes()).await.is_err() {
                    error!("Error writing TCP response");
                    break;
                }
            } else {
                if socket.lock().await.write_all(b"ERROR|Unknown command").await.is_err() {
                    error!("Error writing TCP error response");
                    break;
                }
            }
        }
    }

    async fn handle_udp_socket(self: Arc<Self>) {
        let mut buffer = [0; 2048];
        loop {
            match self.udp_socket.recv_from(&mut buffer).await {
                Ok((n, addr)) => {
                    let command_str = String::from_utf8_lossy(&buffer[..n]);
                    if let Some(command) = Command::from_str(&command_str) {
                        let response = self.handle_command(command, None, Some(addr)).await;
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

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    let online_users = Arc::new(Mutex::new(HashMap::new()));
    let udp_socket = Arc::new(UdpSocket::bind("127.0.0.1:8081").await?);

    let server = Arc::new(Server {
        online_users,
        udp_socket: udp_socket.clone(),
    });

    let tcp_listener = TcpListener::bind("127.0.0.1:8080").await?;

    let tcp_server = server.clone();
    tokio::spawn(async move {
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
    tokio::spawn(async move {
        udp_server.handle_udp_socket().await;
    });

    info!("UDP server running on 127.0.0.1:8081");

    loop {
        time::sleep(Duration::from_secs(60)).await;
    }
}
