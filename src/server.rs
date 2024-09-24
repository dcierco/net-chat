use rand::{distributions::Alphanumeric, Rng};
use std::{collections::HashMap, sync::Arc};
use tokio::fs;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
use log::{info, warn, error};

#[derive(Debug)]
enum Command {
    Register(String, String),
    Authenticate(String, String),
    SendMessage(String, String, String),
    SendFile(String, String, String, usize, usize, Vec<u8>),
    ListUsers(String),
    Logout(String),
}

impl Command {
    fn from_str(command: &str) -> Option<Self> {
        let parts: Vec<&str> = command.split('|').collect();
        match parts[0] {
            "REGISTER" if parts.len() == 3 => Some(Command::Register(parts[1].to_string(), parts[2].to_string())),
            "AUTH" if parts.len() == 3 => Some(Command::Authenticate(parts[1].to_string(), parts[2].to_string())),
            "MESSAGE" if parts.len() == 4 => Some(Command::SendMessage(parts[1].to_string(), parts[2].to_string(), parts[3].to_string())),
            "FILE" if parts.len() >= 6 => {
                let packet_number = parts[4].parse().ok()?;
                let data_size = parts[5].parse().ok()?;
                let data = parts[6..].join("|").into_bytes();
                Some(Command::SendFile(parts[1].to_string(), parts[2].to_string(), parts[3].to_string(), packet_number, data_size, data))
            },
            "LIST_USERS" if parts.len() == 2 => Some(Command::ListUsers(parts[1].to_string())),
            "LOGOUT" if parts.len() == 2 => Some(Command::Logout(parts[1].to_string())),
            _ => None,
        }
    }
}

struct Server {
    user_db: Arc<Mutex<HashMap<String, String>>>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
    online_users: Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>,
    file_transfers: Arc<Mutex<HashMap<String, Vec<Option<Vec<u8>>>>>>,
}

impl Server {
    async fn register_user(&self, username: &str, hashed_password: &str) -> Result<(), String> {
        let mut db = self.user_db.lock().await;
        if db.contains_key(username) {
            Err("Username already taken".into())
        } else {
            db.insert(username.to_string(), hashed_password.to_string());
            info!("User registered: {}", username);
            Ok(())
        }
    }

    async fn authenticate_user(&self, username: &str, hashed_password: &str) -> Result<String, String> {
        let db = self.user_db.lock().await;
        if let Some(stored_hash) = db.get(username) {
            if stored_hash == hashed_password {
                let session_token: String = rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(30)
                    .map(char::from)
                    .collect();
                let mut sessions = self.sessions.lock().await;
                sessions.insert(username.to_string(), session_token.clone());
                info!("User authenticated: {}", username);
                Ok(session_token)
            } else {
                Err("Incorrect password".into())
            }
        } else {
            Err("Username not found".into())
        }
    }

    async fn list_users(&self, username: &str) -> String {
        let sessions = self.sessions.lock().await;
        if !sessions.contains_key(username) {
            return "AUTH|FAILURE|User not authenticated".into();
        }
        let online_users = self.online_users.lock().await;
        let users = online_users.keys().cloned().collect::<Vec<_>>().join(",");
        info!("Listed users for: {}", username);
        format!("LIST_USERS|SUCCESS|{}", users).into()
    }

    async fn logout(&self, username: &str) -> String {
        let mut sessions = self.sessions.lock().await;
        if sessions.remove(username).is_some() {
            let mut online_users = self.online_users.lock().await;
            online_users.remove(username);
            info!("User logged out: {}", username);
            "LOGOUT|SUCCESS".into()
        } else {
            "LOGOUT|FAILURE|User not authenticated".into()
        }
    }

    async fn send_message(&self, from: &str, to: &str, content: &str) -> Result<(), String> {
        let online_users = self.online_users.lock().await;
        if let Some(receiver_socket) = online_users.get(to) {
            let message = format!("MESSAGE|{}|{}", from, content);
            let mut socket_lock = receiver_socket.lock().await;
            if socket_lock.write_all(message.as_bytes()).await.is_err() {
                error!("Failed to send message from {} to {}", from, to);
                return Err("Failed to send message".into());
            }
            info!("Message sent from {} to {}", from, to);
            Ok(())
        } else {
            Err("Recipient not online".into())
        }
    }

    async fn handle_file_transfer(&self, from: &str, to: &str, filename: &str, packet_number: usize, data_size: usize, data: Vec<u8>) -> Result<(), String> {
        let mut file_transfers = self.file_transfers.lock().await;
        let key = format!("{}_{}", from, filename);
        let transfer = file_transfers.entry(key.clone()).or_insert_with(|| vec![None; data_size]);

        if transfer.len() <= packet_number {
            return Err("Invalid packet number".into());
        }

        transfer[packet_number] = Some(data);

        if transfer.iter().all(|packet| packet.is_some()) {
            let mut file_data = Vec::new();
            for packet in transfer.iter_mut() {
                if let Some(data) = packet.take() {
                    file_data.extend_from_slice(&data);
                }
            }

            if let Err(e) = fs::write(filename, &file_data).await {
                error!("Failed to write file: {:?}", e);
                return Err(format!("Failed to write file: {:?}", e));
            }

            file_transfers.remove(&key);
            info!("File successfully transferred: {}", filename);

            let online_users = self.online_users.lock().await;
            if let Some(receiver_socket) = online_users.get(to) {
                let message = format!("FILE|SUCCESS|{}", filename);
                let mut socket_lock = receiver_socket.lock().await;
                if socket_lock.write_all(message.as_bytes()).await.is_err() {
                    error!("Failed to notify recipient: {}", to);
                    return Err("Failed to notify recipient".into());
                }
            } else {
                return Err("Recipient not online".into());
            }
        }

        Ok(())
    }

    async fn handle_tcp_connection(self: Arc<Self>, socket: TcpStream) {
        let socket = Arc::new(Mutex::new(socket));
        let mut buffer = [0; 2048];
        loop {
            let n = match socket.lock().await.read(&mut buffer).await {
                Ok(n) if n == 0 => break,
                Ok(n) => n,
                Err(e) => {
                    warn!("Connection error: {:?}", e);
                    break;
                },
            };
            let command_str = String::from_utf8_lossy(&buffer[..n]);
            if let Some(command) = Command::from_str(&command_str) {
                match command {
                    Command::Register(ref username, ref hashed_password) => {
                        match self.register_user(username, hashed_password).await {
                            Ok(_) => socket.lock().await.write_all(b"REGISTER|SUCCESS").await.unwrap_or_else(|e| error!("Error writing: {:?}", e)),
                            Err(e) => socket.lock().await.write_all(format!("REGISTER|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e)),
                        }
                    }
                    Command::Authenticate(ref username, ref hashed_password) => {
                        match self.authenticate_user(username, hashed_password).await {
                            Ok(token) => {
                                let mut online_users = self.online_users.lock().await;
                                online_users.insert(username.clone(), socket.clone());
                                socket.lock().await.write_all(format!("AUTH|SUCCESS|{}", token).as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                            }
                            Err(e) => socket.lock().await.write_all(format!("AUTH|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e)),
                        }
                    }
                    Command::SendMessage(ref from, ref to, ref content) => {
                        if let Err(e) = self.send_message(from, to, content).await {
                            socket.lock().await.write_all(format!("MESSAGE|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                        } else {
                            socket.lock().await.write_all(b"MESSAGE|SUCCESS").await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                        }
                    }
                    Command::SendFile(ref from, ref to, ref filename, packet_number, data_size, data) => {
                        if let Err(e) = self.handle_file_transfer(from, to, filename, packet_number, data_size, data).await {
                            socket.lock().await.write_all(format!("FILE|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                        } else {
                            socket.lock().await.write_all(b"FILE|SUCCESS").await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                        }
                    }
                    Command::ListUsers(ref username) => {
                        let response = self.list_users(username).await;
                        socket.lock().await.write_all(response.as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                    }
                    Command::Logout(ref username) => {
                        let response = self.logout(username).await;
                        socket.lock().await.write_all(response.as_bytes()).await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
                        break;
                    }
                }
            } else {
                socket.lock().await.write_all(b"ERROR|Unknown command").await.unwrap_or_else(|e| error!("Error writing: {:?}", e));
            }
        }
    }

    async fn handle_udp_socket(self: Arc<Self>, socket: UdpSocket) {
        let mut buffer = [0; 2048];
        loop {
            let (n, addr) = match socket.recv_from(&mut buffer).await {
                Ok((n, addr)) => (n, addr),
                Err(e) => {
                    warn!("UDP receive error: {:?}", e);
                    continue;
                },
            };

            let packet_info = String::from_utf8_lossy(&buffer[..n]);
            let parts: Vec<&str> = packet_info.split('|').collect();
            if parts.len() < 5 {
                warn!("Invalid packet format from {:?}", addr);
                continue;
            }

            let from = parts[0];
            let to = parts[1];
            let filename = parts[2];
            let packet_number: usize = match parts[3].parse() {
                Ok(num) => num,
                Err(_) => {
                    warn!("Invalid packet number from {:?}", addr);
                    continue;
                },
            };
            let data_size: usize = match parts[4].parse() {
                Ok(size) => size,
                Err(_) => {
                    warn!("Invalid data size from {:?}", addr);
                    continue;
                },
            };
            let data = buffer[packet_info.len()..packet_info.len() + data_size].to_vec();

            if let Err(e) = self.handle_file_transfer(from, to, filename, packet_number, data_size, data).await {
                error!("Error handling file transfer: {:?}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    let user_db = Arc::new(Mutex::new(HashMap::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let online_users = Arc::new(Mutex::new(HashMap::new()));
    let file_transfers: Arc<Mutex<HashMap<_, _>>> = Arc::new(Mutex::new(HashMap::new()));

    let server = Arc::new(Server { user_db, sessions, online_users, file_transfers });

    let tcp_listener = TcpListener::bind("127.0.0.1:8080").await?;
    let udp_socket = UdpSocket::bind("127.0.0.1:8081").await?;

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
        udp_server.handle_udp_socket(udp_socket).await;
    });

    info!("UDP server running on 127.0.0.1:8081");

    loop {
        time::sleep(Duration::from_secs(60)).await;
    }
}
