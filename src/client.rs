use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{self, AsyncWriteExt, AsyncReadExt};
use tokio::sync::{mpsc, Mutex};
use std::env;
use std::sync::Arc;
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    Tcp,
    Udp,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Register(String),
    SendMessage(String, String, String, Transport),
    ListUsers(String),
    Disconnect(String),
}

async fn client_main(server_address: &str, client_address: &str) -> io::Result<()> {
    let tcp_stream = Arc::new(Mutex::new(TcpStream::connect(server_address).await?));
    let udp_socket = Arc::new(UdpSocket::bind(client_address).await?);
    udp_socket.connect(server_address).await?;

    let (tx, mut rx) = mpsc::channel(100);
    let udp_socket_clone = Arc::clone(&udp_socket);

    // Spawn a task to handle incoming UDP messages
    tokio::spawn(async move {
        let mut buf = [0; 1024];
        loop {
            match udp_socket_clone.recv_from(&mut buf).await {
                Ok((len, _)) => {
                    println!("Received UDP: {}", String::from_utf8_lossy(&buf[..len]));
                }
                Err(e) => eprintln!("UDP receive error: {}", e),
            }
        }
    });

    // Spawn a task to handle incoming TCP messages
    let tcp_stream_clone = Arc::clone(&tcp_stream);
    tokio::spawn(async move {
        let mut buf = [0; 1024];
        loop {
            let mut locked_stream = tcp_stream_clone.lock().await;
            match locked_stream.read(&mut buf).await {
                Ok(0) => break, // Connection closed
                Ok(n) => {
                    println!("Received TCP: {}", String::from_utf8_lossy(&buf[..n]));
                }
                Err(e) => {
                    eprintln!("TCP receive error: {}", e);
                    break;
                }
            }
        }
    });

    let mut current_user: Option<String> = None;

    loop {
        tokio::select! {
            Some(command) = rx.recv() => {
                handle_command(command, &tcp_stream, &udp_socket, &mut current_user).await?;
            }
            result = get_user_input() => {
                match result {
                    Ok(command) => {
                        tx.send(command).await.unwrap();
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
        }
    }
}

pub trait UdpSocketLike {
    fn send<'a>(&'a self, buf: &'a [u8]) -> Pin<Box<dyn Future<Output = io::Result<usize>> + Send + 'a>>;
}

impl UdpSocketLike for Arc<UdpSocket> {
    fn send<'a>(&'a self, buf: &'a [u8]) -> Pin<Box<dyn Future<Output = io::Result<usize>> + Send + 'a>> {
        Box::pin(UdpSocket::send(self, buf))
    }
}

pub async fn handle_command<T, U>(
    command: Command,
    tcp_stream: &Arc<tokio::sync::Mutex<T>>,
    udp_socket: &U,
    current_user: &mut Option<String>,
) -> io::Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
    U: UdpSocketLike,
{
    match command {
        Command::Register(username) => {
            let cmd = format!("REGISTER|{}", username);
            tcp_stream.lock().await.write_all(cmd.as_bytes()).await?;
            *current_user = Some(username);
        }
        Command::SendMessage(from, to, content, transport) => {
            let cmd = format!("MESSAGE|{}|{}|{}|{}", from, to, content,
                              match transport {
                                  Transport::Tcp => "TCP",
                                  Transport::Udp => "UDP",
                              });
            match transport {
                Transport::Tcp => tcp_stream.lock().await.write_all(cmd.as_bytes()).await?,
                Transport::Udp => { udp_socket.send(cmd.as_bytes()).await?; }
            }
        }
        Command::ListUsers(username) => {
            let cmd = format!("LIST_USERS|{}", username);
            tcp_stream.lock().await.write_all(cmd.as_bytes()).await?;
        }
        Command::Disconnect(username) => {
            let cmd = format!("DISCONNECT|{}", username);
            tcp_stream.lock().await.write_all(cmd.as_bytes()).await?;
            *current_user = None;
        }
    }
    Ok(())
}

pub async fn get_user_input() -> Result<Command, &'static str> {
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();

    let parts: Vec<&str> = input.split_whitespace().collect();
    match parts[0] {
        "register" if parts.len() == 2 => Ok(Command::Register(parts[1].to_string())),
        "message" if parts.len() >= 4 => {
            let from = parts[1].to_string();
            let to = parts[2].to_string();
            let content = parts[3..].join(" ");
            let transport = if parts.last().unwrap() == &"UDP" {
                Transport::Udp
            } else {
                Transport::Tcp
            };
            Ok(Command::SendMessage(from, to, content, transport))
        }
        "list" if parts.len() == 2 => Ok(Command::ListUsers(parts[1].to_string())),
        "disconnect" if parts.len() == 2 => Ok(Command::Disconnect(parts[1].to_string())),
        _ => Err("Invalid command. Use: register <username>, message <from> <to> <content> [UDP], list <username>, or disconnect <username>"),
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: client <server_address> <client_address>");
        return Ok(());
    }
    let server_address: &String = &args[1];
    let client_address: &String = &args[2];

    client_main(server_address, client_address).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use std::task::{Context, Poll};
    use std::sync::Mutex;

    struct MockUdpSocket {
        tx: mpsc::Sender<String>,
        sent_messages: Mutex<Vec<String>>,
    }

    impl MockUdpSocket {
        fn new(tx: mpsc::Sender<String>) -> Self {
            MockUdpSocket {
                tx,
                sent_messages: Mutex::new(Vec::new()),
            }
        }

        async fn send(&self, buf: &[u8]) -> io::Result<usize> {
            let msg = String::from_utf8_lossy(buf).to_string();
            self.tx.send(msg.clone()).await.unwrap();
            self.sent_messages.lock().unwrap().push(msg);
            Ok(buf.len())
        }

        fn get_sent_messages(&self) -> Vec<String> {
            self.sent_messages.lock().unwrap().clone()
        }
    }

    impl UdpSocketLike for MockUdpSocket {
        fn send<'a>(&'a self, buf: &'a [u8]) -> Pin<Box<dyn Future<Output = io::Result<usize>> + Send + 'a>> {
            Box::pin(self.send(buf))
        }
    }

    #[tokio::test]
    async fn test_get_user_input() {
        let inputs = vec![
            ("register alice", Ok(Command::Register("alice".to_string()))),
            ("message alice bob hello", Ok(Command::SendMessage("alice".to_string(), "bob".to_string(), "hello".to_string(), Transport::Tcp))),
            ("message alice bob hello UDP", Ok(Command::SendMessage("alice".to_string(), "bob".to_string(), "hello UDP".to_string(), Transport::Udp))),
            ("list alice", Ok(Command::ListUsers("alice".to_string()))),
            ("disconnect alice", Ok(Command::Disconnect("alice".to_string()))),
            ("invalid command", Err("Invalid command. Use: register <username>, message <from> <to> <content> [UDP], list <username>, or disconnect <username>")),
        ];

        for (input, expected_output) in inputs {
            let result = get_user_input_test(input).await;
            assert_eq!(result, expected_output);
        }
    }

    async fn get_user_input_test(input: &str) -> Result<Command, &'static str> {
        let input = input.to_string();
        get_user_input_from_string(input).await
    }

    async fn get_user_input_from_string(input: String) -> Result<Command, &'static str> {
        let input = input.trim();
        get_user_input_impl(input)
    }

    fn get_user_input_impl(input: &str) -> Result<Command, &'static str> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts[0] {
            "register" if parts.len() == 2 => Ok(Command::Register(parts[1].to_string())),
            "message" if parts.len() >= 4 => {
                let from = parts[1].to_string();
                let to = parts[2].to_string();
                let content = parts[3..].join(" ");
                let transport = if parts.last().unwrap() == &"UDP" {
                    Transport::Udp
                } else {
                    Transport::Tcp
                };
                Ok(Command::SendMessage(from, to, content, transport))
            }
            "list" if parts.len() == 2 => Ok(Command::ListUsers(parts[1].to_string())),
            "disconnect" if parts.len() == 2 => Ok(Command::Disconnect(parts[1].to_string())),
            _ => Err("Invalid command. Use: register <username>, message <from> <to> <content> [UDP], list <username>, or disconnect <username>"),
        }
    }

    #[tokio::test]
    async fn test_handle_command() {
        let (tx, mut rx) = mpsc::channel(10);
        let tcp_stream = Arc::new(tokio::sync::Mutex::new(MockTcpStream { tx: tx.clone() }));
        let udp_socket = MockUdpSocket::new(tx);
        let mut current_user = None;

        // Test Register command
        let cmd = Command::Register("alice".to_string());
        handle_command(cmd, &tcp_stream, &udp_socket, &mut current_user).await.unwrap();
        assert_eq!(current_user, Some("alice".to_string()));
        assert_eq!(rx.try_recv().unwrap(), "REGISTER|alice");

        // Test SendMessage command (TCP)
        let cmd = Command::SendMessage("alice".to_string(), "bob".to_string(), "hello".to_string(), Transport::Tcp);
        handle_command(cmd, &tcp_stream, &udp_socket, &mut current_user).await.unwrap();
        assert_eq!(rx.try_recv().unwrap(), "MESSAGE|alice|bob|hello|TCP");

        // Test SendMessage command (UDP)
        let cmd = Command::SendMessage("alice".to_string(), "bob".to_string(), "hello".to_string(), Transport::Udp);
        handle_command(cmd, &tcp_stream, &udp_socket, &mut current_user).await.unwrap();
        assert_eq!(rx.try_recv().unwrap(), "MESSAGE|alice|bob|hello|UDP");

        // Test ListUsers command
        let cmd = Command::ListUsers("alice".to_string());
        handle_command(cmd, &tcp_stream, &udp_socket, &mut current_user).await.unwrap();
        assert_eq!(rx.try_recv().unwrap(), "LIST_USERS|alice");

        // Test Disconnect command
        let cmd = Command::Disconnect("alice".to_string());
        handle_command(cmd, &tcp_stream, &udp_socket, &mut current_user).await.unwrap();
        assert_eq!(current_user, None);
        assert_eq!(rx.try_recv().unwrap(), "DISCONNECT|alice");

        // Check sent UDP messages
        let sent_udp_messages = udp_socket.get_sent_messages();
        assert_eq!(sent_udp_messages, vec!["MESSAGE|alice|bob|hello|UDP"]);
    }

    struct MockTcpStream {
        tx: mpsc::Sender<String>,
    }

    impl AsyncRead for MockTcpStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
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
            self.tx.try_send(msg).unwrap();
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            Poll::Ready(Ok(()))
        }
    }
}
