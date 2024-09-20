use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use rand::{Rng, distributions::Alphanumeric};

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
    online_users: Arc<Mutex<HashMap<String, TcpStream>>>, // Username to TcpStream hashmap
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

    async fn logout(&self, username: String) -> String {
        let mut sessions = self.sessions.lock().await;
        if sessions.remove(&username).is_some() {
            let mut online_users = self.online_users.lock().await;
            online_users.remove(&username);
            "LOGOUT|SUCCESS".into()
        } else {
            "LOGOUT|FAILURE|User not authenticated".into()
        }
    }

    async fn handle_tcp_connection(self: Arc<Self>, socket: TcpStream) {
        let mut socket = socket; // Unused mut warning resolved by re-assignment
        let mut buffer = [0; 1024];
        loop {
            let n = match socket.read(&mut buffer).await {
                Ok(n) if n == 0 => break, // Connection closed
                Ok(n) => n,
                Err(_) => break,
            };
            let command_str = String::from_utf8_lossy(&buffer[..n]);
            if let Some(command) = Command::from_str(&command_str) {
                match command {
                    Command::Register(username, hashed_password) => {
                        match self.register_user(username, hashed_password).await {
                            Ok(_) => socket.write_all(b"REGISTER|SUCCESS").await.expect("Failed to write data"),
                            Err(e) => socket.write_all(format!("REGISTER|FAILURE|{}", e).as_bytes()).await.expect("Failed to write data"),
                        }
                    }
                    Command::Authenticate(username, hashed_password) => {
                        match self.authenticate_user(username.clone(), hashed_password).await {
                            Ok(token) => {
                                self.online_users.lock().await.insert(username.clone(), socket.try_clone().unwrap());
                                socket.write_all(format!("AUTH|SUCCESS|{}", token).as_bytes()).await.expect("Failed to write data");
                            },
                            Err(e) => socket.write_all(format!("AUTH|FAILURE|{}", e).as_bytes()).await.expect("Failed to write data"),
                        }
                    }
                    Command::SendMessage(from, to, content) => {
                        // Handle message sending...
                        // E.g., ensure `to` is an online user, then send the message
                    }
                    Command::SendFile(from, to, filename, packet_number, data_size, data) => {
                        // Handle file transfer...
                    }
                    Command::ListUsers(username) => {
                        let response = self.list_users(username).await;
                        socket.write_all(response.as_bytes()).await.expect("Failed to write data");
                    }
                    Command::Logout(username) => {
                        let response = self.logout(username).await;
                        socket.write_all(response.as_bytes()).await.expect("Failed to write data");
                        break; // Close connection after logout
                    }
                }
            } else {
                socket.write_all(b"ERROR|Unknown command").await.expect("Failed to write data");
            }
        }
    }

    async fn handle_udp_socket(self: Arc<Self>, socket: UdpSocket) {
        let mut buffer = [0; 1048]; // Extra for packet meta
        loop {
            let (n, _addr) = match socket.recv_from(&mut buffer).await {
                Ok((n, addr)) => (n, addr),
                Err(_) => continue,
            };
            let packet_info = String::from_utf8_lossy(&buffer[..n]);
            let parts: Vec<&str> = packet_info.split('|').collect();
            if parts.len() < 3 {
                continue; // Invalid packet format
            }
            let packet_number: usize = match parts[0].parse() {
                Ok(num) => num,
                Err(_) => continue,
            };
            let data_size: usize = match parts[1].parse() {
                Ok(size) => size,
                Err(_) => continue,
            };
            let data = buffer[2..2 + data_size].to_vec();
            // Handle file transfer...
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let user_db = Arc::new(Mutex::new(HashMap::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let online_users = Arc::new(Mutex::new(HashMap::new()));
    let server = Arc::new(Server { user_db, sessions, online_users });

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
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}