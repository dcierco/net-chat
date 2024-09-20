use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use rand::{Rng, distributions::Alphanumeric};
use tokio::time::{self, Duration};
use tokio::fs;

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
    user_db: Arc<Mutex<HashMap<String, String>>>, // Username to password hashmap
    sessions: Arc<Mutex<HashMap<String, String>>>, // Username to session token hashmap
    online_users: Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>, // Username to TcpStream hashmap
    file_transfers: Arc<Mutex<HashMap<String, Vec<Option<Vec<u8>>>>>>, // Track file transfers
}

impl Server {
    async fn register_user(&self, username: String, hashed_password: String) -> Result<(), String> {
        let mut db = self.user_db.lock().await;
        if db.contains_key(&username) {
            Err("Username already taken".into())
        } else {
            db.insert(username, hashed_password);
            Ok(())
        }
    }

    async fn authenticate_user(&self, username: String, hashed_password: String) -> Result<String, String> {
        let db = self.user_db.lock().await;
        if let Some(stored_hash) = db.get(&username) {
            if stored_hash == &hashed_password {
                let session_token: String = rand::thread_rng().sample_iter(&Alphanumeric).take(30).map(char::from).collect();
                let mut sessions = self.sessions.lock().await;
                sessions.insert(username.clone(), session_token.clone());
                Ok(session_token)
            } else {
                Err("Incorrect password".into())
            }
        } else {
            Err("Username not found".into())
        }
    }
    
    async fn list_users(&self, username: String) -> String {
        let sessions = self.sessions.lock().await;
        if !sessions.contains_key(&username) {
            return "AUTH|FAILURE|User not authenticated".into();
        }

        let online_users = self.online_users.lock().await;
        let users = online_users.keys().cloned().collect::<Vec<_>>().join(",");
        format!("LIST_USERS|SUCCESS|{}", users)
    }

    async fn logout(&self, username: &str) -> String {
        let mut sessions = self.sessions.lock().await;
        if sessions.remove(username).is_some() {
            let mut online_users = self.online_users.lock().await;
            online_users.remove(username);
            "LOGOUT|SUCCESS".into()
        } else {
            "LOGOUT|FAILURE|User not authenticated".into()
        }
    }

    async fn send_message(&self, from: String, to: String, content: String) -> Result<(), String> {
        let online_users = self.online_users.lock().await;
        if let Some(receiver_socket) = online_users.get(&to) {
            let message = format!("MESSAGE|{}|{}", from, content);
            let mut socket_lock = receiver_socket.lock().await;
            if let Err(_) = socket_lock.write_all(message.as_bytes()).await {
                return Err("Failed to send message".into());
            }
            Ok(())
        } else {
            Err("Recipient not online".into())
        }
    }

async fn handle_file_transfer(&self, from: String, to: String, filename: String, packet_number: usize, data_size: usize, data: Vec<u8>) -> Result<(), String> {
        let mut file_transfers = self.file_transfers.lock().await;

        let key = format!("{}_{}", from, filename);

        let transfer = file_transfers.entry(key.clone()).or_insert_with(|| vec![None; data_size]); // Initialize with None

        if transfer.len() <= packet_number {
            return Err("Invalid packet number".into());
        }

        transfer[packet_number] = Some(data);

        if transfer.iter().all(|packet| packet.is_some()) {
            // All packets received

            let mut file_data = Vec::new();
            for packet in transfer {
                if let Some(data) = packet {
                    file_data.extend_from_slice(data);
                }
            }

            // Save file
            if let Err(e) = fs::write(&filename, &file_data).await {
                return Err(format!("Failed to write file: {:?}", e));
            }

            file_transfers.remove(&key);

            // Notify the recipient
            let online_users = self.online_users.lock().await;
            if let Some(receiver_socket) = online_users.get(&to) {
                let message = format!("FILE|SUCCESS|{}", filename);
                let mut socket_lock = receiver_socket.lock().await;
                if let Err(_) = socket_lock.write_all(message.as_bytes()).await {
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
        let mut buffer = [0; 1024];
        loop {
            let n = match socket.lock().await.read(&mut buffer).await {
                Ok(n) if n == 0 => break, // Connection closed
                Ok(n) => n,
                Err(_) => break,
            };
            let command_str = String::from_utf8_lossy(&buffer[..n]);
            if let Some(command) = Command::from_str(&command_str) {
                match command {
                    Command::Register(username, hashed_password) => {
                        match self.register_user(username, hashed_password).await {
                            Ok(_) => socket.lock().await.write_all(b"REGISTER|SUCCESS").await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e)),
                            Err(e) => socket.lock().await.write_all(format!("REGISTER|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e)),
                        }
                    }
                    Command::Authenticate(username, hashed_password) => {
                        match self.authenticate_user(username.clone(), hashed_password).await {
                            Ok(token) => {
                                let mut online_users = self.online_users.lock().await;
                                online_users.insert(username.clone(), socket.clone());
                                socket.lock().await.write_all(format!("AUTH|SUCCESS|{}", token).as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                            }
                            Err(e) => socket.lock().await.write_all(format!("AUTH|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e)),
                        }
                    }
                    Command::SendMessage(from, to, content) => {
                        if let Err(e) = self.send_message(from, to, content).await {
                            socket.lock().await.write_all(format!("MESSAGE|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                        } else {
                            socket.lock().await.write_all(b"MESSAGE|SUCCESS").await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                        }
                    }
                    Command::SendFile(from, to, filename, packet_number, data_size, data) => {
                        if let Err(e) = self.handle_file_transfer(from, to, filename, packet_number, data_size, data).await {
                            socket.lock().await.write_all(format!("FILE|FAILURE|{}", e).as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                        } else {
                            socket.lock().await.write_all(b"FILE|SUCCESS").await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                        }
                    }
                    Command::ListUsers(username) => {
                        let response = self.list_users(username).await;
                        socket.lock().await.write_all(response.as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                    }
                    Command::Logout(username) => {
                        let response = self.logout(&username).await;
                        socket.lock().await.write_all(response.as_bytes()).await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
                        break; // Close connection after logout
                    }
                }
            } else {
                socket.lock().await.write_all(b"ERROR|Unknown command").await.unwrap_or_else(|e| eprintln!("Error writing: {:?}", e));
            }
        }
    }

async fn handle_udp_socket(self: Arc<Self>, socket: UdpSocket) {
    let mut buffer = [0; 1500]; // Buffer size for UDP packets
    loop {
        let (n, addr) = match socket.recv_from(&mut buffer).await {
            Ok((n, addr)) => (n, addr),
            Err(_) => continue,
        };

        let packet_info = String::from_utf8_lossy(&buffer[..n]);
        let parts: Vec<&str> = packet_info.split('|').collect();
        if parts.len() < 5 {
            eprintln!("Invalid packet format from {:?}", addr);
            continue;
        }

        let from = parts[0].to_string();
        let to = parts[1].to_string();
        let filename = parts[2].to_string();
        let packet_number: usize = match parts[3].parse() {
            Ok(num) => num,
            Err(_) => continue,
        };
        let data_size: usize = match parts[4].parse() {
            Ok(size) => size,
            Err(_) => continue,
        };
        let data = buffer[packet_info.len()..packet_info.len() + data_size].to_vec();

        if let Err(e) = self.handle_file_transfer(from, to, filename, packet_number, data_size, data).await {
            eprintln!("Error handling file transfer: {:?}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let user_db = Arc::new(Mutex::new(HashMap::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let online_users = Arc::new(Mutex::new(HashMap::new()));
    let file_transfers: Arc<Mutex<HashMap<_, _>>> = Arc::new(Mutex::new(HashMap::new()));

    let server = Arc::new(Server { user_db, sessions, online_users, file_transfers});

    let tcp_listener = TcpListener::bind("127.0.0.1:8080").await?;
    let udp_socket = UdpSocket::bind("127.0.0.1:8081").await?;

    let tcp_server = server.clone();
    tokio::spawn(async move {
        loop {
            let (socket, _) = tcp_listener.accept().await.unwrap();
            let tcp_server = tcp_server.clone();
            tokio::spawn(async move {
                tcp_server.handle_tcp_connection(socket).await;
            });
        }
    });

    let udp_server = server.clone();
    tokio::spawn(async move {
        udp_server.handle_udp_socket(udp_socket).await;
    });

    // Keep the main function running
    loop {
        time::sleep(Duration::from_secs(60)).await;
    }
}