use crate::common::{receive_command, send_command, send_server_response, Command, ServerResponse};
use crate::udp::run_udp_server;
use log::{debug, error, info};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

type ClientMap = Arc<Mutex<HashMap<String, BufWriter<TcpStream>>>>;

pub fn setup_server(address: &str) -> std::io::Result<(ClientMap, Sender<()>)> {
    info!("Setting up server on {}", address);
    let clients: ClientMap = Arc::new(Mutex::new(HashMap::new()));
    let (tx, _) = std::sync::mpsc::channel();
    Ok((clients, tx))
}

pub fn run_server(
    tcp_listener: Arc<TcpListener>,
    udp_socket: Arc<UdpSocket>,
    clients: ClientMap,
    udp_clients: Arc<Mutex<HashMap<String, std::net::SocketAddr>>>,
    rx: Receiver<()>,
    set_ctrl_c: bool,
) {
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();

    // Spawn a new thread for UDP handling
    let udp_clients_clone = Arc::clone(&udp_clients);
    let udp_socket_clone = Arc::clone(&udp_socket);
    let running_udp = Arc::clone(&running);
    thread::spawn(move || {
        if let Ok(socket) = (*udp_socket_clone).try_clone() {
            run_udp_server(socket, udp_clients_clone, running_udp);
        } else {
            error!("Failed to clone UDP socket for UDP server thread");
        }
    });

    // Set up a ctrl-c handler only if requested
    if set_ctrl_c {
        ctrlc::set_handler(move || {
            println!("Shutting down server...");
            r.store(false, std::sync::atomic::Ordering::SeqCst);
        })
        .expect("Error setting Ctrl-C handler");
    }

    // Set the listener to non-blocking mode
    tcp_listener
        .set_nonblocking(true)
        .expect("Cannot set non-blocking");

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        match tcp_listener.accept() {
            Ok((stream, addr)) => {
                info!("New connection: {}", addr);
                let clients = Arc::clone(&clients);
                let running = Arc::clone(&running);
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, clients, running) {
                        error!("Error handling client {}: {}", addr, e);
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No pending connections, sleep for a short while
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }

        // Check for shutdown signal
        if rx.try_recv().is_ok() {
            info!("Shutdown signal received");
            break;
        }
    }

    info!("Server shutting down");
}

fn handle_client(
    stream: TcpStream,
    clients: ClientMap,
    running: Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);
    let mut name = String::new();

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        match receive_command(&mut reader)? {
            Some(command) => {
                debug!("Received command: {:?}", command);
                match command {
                    Command::Register(client_name) => {
                        name = client_name.clone();
                        clients
                            .lock()
                            .unwrap()
                            .insert(client_name, BufWriter::new(writer.get_ref().try_clone()?));
                        info!("Registered client: {}", name);
                        send_server_response(&mut writer, &ServerResponse::RegistrationSuccessful)?;
                    }
                    Command::Scream(message) => {
                        if name.is_empty() {
                            send_command(
                                &mut writer,
                                &Command::Scream("Please register first".to_string()),
                            )?;
                            writer.flush()?;
                        } else {
                            info!("Received broadcast from {}: {}", name, message);
                            broadcast_message(&clients, &name, &message)?;
                        }
                    }
                    Command::Whisper(recipient, message) => {
                        if name.is_empty() {
                            send_command(
                                &mut writer,
                                &Command::Scream("Please register first".to_string()),
                            )?;
                            writer.flush()?;
                        } else {
                            info!(
                                "Received whisper from {} to {}: {}",
                                name, recipient, message
                            );
                            whisper_message(&clients, &name, &recipient, &message)?;
                        }
                    }
                    Command::SendFile(recipient, filename, content) => {
                        if name.is_empty() {
                            send_server_response(
                                &mut writer,
                                &ServerResponse::Message("Please register first".to_string()),
                            )?;
                        } else {
                            let file_size = content.len();
                            let file_hash = calculate_hash(&content);
                            info!("File transfer initiated: {} is sending '{}' to {} (Size: {} bytes, Hash: {:x})",
                                  name, filename, recipient, file_size, file_hash);

                            handle_file_transfer(
                                &clients,
                                &name,
                                &recipient,
                                &filename,
                                &content,
                                &mut writer,
                            )?;

                            info!("File transfer completed: '{}' from {} to {} (Size: {} bytes, Hash: {:x})",
                                  filename, name, recipient, file_size, file_hash);
                        }
                    }
                    Command::ListUsers => {
                        let user_list: Vec<String> =
                            clients.lock().unwrap().keys().cloned().collect();
                        send_server_response(&mut writer, &ServerResponse::UserList(user_list))?;
                        writer.flush()?;
                    }
                    Command::Disconnect => {
                        info!("Client {} disconnected", name);
                        clients.lock().unwrap().remove(&name);
                        break;
                    }
                }
            }
            None => {
                if !name.is_empty() {
                    info!("Client {} disconnected unexpectedly", name);
                    clients.lock().unwrap().remove(&name);
                }
                break;
            }
        }
    }
    Ok(())
}

fn broadcast_message(clients: &ClientMap, sender: &str, message: &str) -> std::io::Result<()> {
    let broadcast_message = format!("{}: {}", sender, message);
    let mut clients = clients.lock().unwrap();
    for (client_name, client_writer) in clients.iter_mut() {
        if client_name != sender {
            send_command(client_writer, &Command::Scream(broadcast_message.clone()))?;
            client_writer.flush()?;
        }
    }
    Ok(())
}

fn whisper_message(
    clients: &ClientMap,
    sender: &str,
    recipient: &str,
    message: &str,
) -> std::io::Result<()> {
    let mut clients = clients.lock().unwrap();
    if let Some(client_writer) = clients.get_mut(recipient) {
        send_command(
            client_writer,
            &Command::Whisper(sender.to_string(), message.to_string()),
        )?;
        client_writer.flush()?;
        Ok(())
    } else {
        let sender_writer = clients.get_mut(sender).unwrap();
        send_server_response(
            sender_writer,
            &ServerResponse::Message(format!("User {} not found", recipient)),
        )?;
        sender_writer.flush()?;
        Ok(())
    }
}

fn handle_file_transfer(
    clients: &ClientMap,
    sender: &str,
    recipient: &str,
    filename: &str,
    content: &[u8],
    writer: &mut impl Write,
) -> std::io::Result<()> {
    let mut clients = clients.lock().unwrap();
    if let Some(recipient_writer) = clients.get_mut(recipient) {
        send_server_response(
            recipient_writer,
            &ServerResponse::FileTransferStarted(filename.to_string()),
        )?;
        send_command(
            recipient_writer,
            &Command::SendFile(sender.to_string(), filename.to_string(), content.to_vec()),
        )?;
        send_server_response(
            recipient_writer,
            &ServerResponse::FileTransferComplete(filename.to_string()),
        )?;
        recipient_writer.flush()?;
        send_server_response(
            writer,
            &ServerResponse::Message(format!("File sent to {}", recipient)),
        )?;
        info!(
            "File '{}' successfully transferred from {} to {}",
            filename, sender, recipient
        );
    } else {
        send_server_response(
            writer,
            &ServerResponse::FileTransferFailed(format!("User {} not found", recipient)),
        )?;
        info!("File transfer failed: recipient {} not found", recipient);
    }
    Ok(())
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::receive_message;
    use crate::initialize;
    use std::io::Write;
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::thread;
    use std::time::{Duration, Instant};

    // In the tests module, add this function:
    fn connect_with_retry(
        addr: std::net::SocketAddr,
        max_attempts: u32,
    ) -> std::io::Result<TcpStream> {
        for attempt in 1..=max_attempts {
            match TcpStream::connect(addr) {
                Ok(stream) => return Ok(stream),
                Err(e) if attempt == max_attempts => return Err(e),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        unreachable!()
    }

    #[test]
    fn test_multiple_clients() {
        initialize();

        let (tx, rx) = channel();
        let (shutdown_tx, shutdown_rx) = channel();

        let server_thread = thread::spawn(move || {
            let tcp_listener = Arc::new(TcpListener::bind("127.0.0.1:0").unwrap());
            let addr = tcp_listener.local_addr().unwrap();
            let udp_socket = Arc::new(UdpSocket::bind(addr).unwrap());
            tx.send(addr).unwrap();
            let (clients, _) = setup_server(&addr.to_string()).unwrap();
            let udp_clients = Arc::new(Mutex::new(HashMap::new()));
            run_server(
                Arc::clone(&tcp_listener),
                Arc::clone(&udp_socket),
                clients,
                udp_clients,
                shutdown_rx,
                false,
            );
        });

        let server_addr = rx.recv().unwrap();
        thread::sleep(Duration::from_millis(100));

        let client1 = TcpStream::connect(server_addr).unwrap();
        let client2 = TcpStream::connect(server_addr).unwrap();

        let mut reader1 = BufReader::new(client1.try_clone().unwrap());
        let mut reader2 = BufReader::new(client2.try_clone().unwrap());
        let mut writer1 = BufWriter::new(client1.try_clone().unwrap());
        let mut writer2 = BufWriter::new(client2.try_clone().unwrap());

        // Register clients
        send_command(&mut writer1, &Command::Register("Alice".to_string())).unwrap();
        send_command(&mut writer2, &Command::Register("Bob".to_string())).unwrap();

        // Wait for registration confirmations
        assert!(matches!(
            receive_message(&mut reader1),
            Ok(Some(Err(ServerResponse::RegistrationSuccessful)))
        ));
        assert!(matches!(
            receive_message(&mut reader2),
            Ok(Some(Err(ServerResponse::RegistrationSuccessful)))
        ));

        // Send a message from Alice
        send_command(&mut writer1, &Command::Scream("Hello, Bob!".to_string())).unwrap();

        // Check if Bob received the message
        match receive_message(&mut reader2) {
            Ok(Some(Ok(Command::Scream(msg)))) => assert_eq!(msg, "Alice: Hello, Bob!"),
            other => panic!("Unexpected response: {:?}", other),
        }

        // Disconnect clients
        send_command(&mut writer1, &Command::Disconnect).unwrap();
        send_command(&mut writer2, &Command::Disconnect).unwrap();

        // Stop the server
        shutdown_tx.send(()).unwrap();
        server_thread.join().unwrap();
    }

    #[test]
    fn test_list_users() {
        println!("Starting test_list_users");
        initialize();

        let (tx, rx) = channel();
        let (shutdown_tx, shutdown_rx) = channel();

        let server_thread = thread::spawn(move || {
            let tcp_listener = Arc::new(TcpListener::bind("127.0.0.1:0").unwrap());
            let addr = tcp_listener.local_addr().unwrap();
            let udp_socket = Arc::new(UdpSocket::bind(addr).unwrap());
            tx.send(addr).unwrap();
            let (clients, _) = setup_server(&addr.to_string()).unwrap();
            let udp_clients = Arc::new(Mutex::new(HashMap::new()));
            run_server(
                Arc::clone(&tcp_listener),
                Arc::clone(&udp_socket),
                clients,
                udp_clients,
                shutdown_rx,
                false,
            );
        });

        let server_addr = rx.recv().unwrap();
        thread::sleep(Duration::from_millis(100));

        let client1 = connect_with_retry(server_addr, 5).unwrap();
        let client2 = connect_with_retry(server_addr, 5).unwrap();

        let mut reader1 = BufReader::new(client1.try_clone().unwrap());
        let mut reader2 = BufReader::new(client2.try_clone().unwrap());
        let mut writer1 = BufWriter::new(client1.try_clone().unwrap());
        let mut writer2 = BufWriter::new(client2.try_clone().unwrap());

        // Register clients
        send_command(&mut writer1, &Command::Register("Alice".to_string())).unwrap();
        writer1.flush().unwrap();
        send_command(&mut writer2, &Command::Register("Bob".to_string())).unwrap();
        writer2.flush().unwrap();

        // Wait for registration confirmations
        assert!(matches!(
            receive_message(&mut reader1),
            Ok(Some(Err(ServerResponse::RegistrationSuccessful)))
        ));
        assert!(matches!(
            receive_message(&mut reader2),
            Ok(Some(Err(ServerResponse::RegistrationSuccessful)))
        ));

        // Request user list
        send_command(&mut writer1, &Command::ListUsers).unwrap();
        writer1.flush().unwrap();

        // Check the response
        match receive_message(&mut reader1) {
            Ok(Some(Err(ServerResponse::UserList(users)))) => {
                assert!(users.contains(&"Alice".to_string()));
                assert!(users.contains(&"Bob".to_string()));
                assert_eq!(users.len(), 2);
            }
            other => panic!(
                "Did not receive expected UserList response. Got: {:?}",
                other
            ),
        }

        // Disconnect clients
        send_command(&mut writer1, &Command::Disconnect).unwrap();
        writer1.flush().unwrap();
        send_command(&mut writer2, &Command::Disconnect).unwrap();
        writer2.flush().unwrap();

        // Stop the server
        shutdown_tx.send(()).unwrap();
        server_thread.join().unwrap();
        println!("test_list_users completed successfully");
    }

    #[test]
    fn test_file_transfer() {
        initialize();

        let (tx, rx) = channel();
        let (shutdown_tx, shutdown_rx) = channel();

        let server_thread = thread::spawn(move || {
            let tcp_listener = Arc::new(TcpListener::bind("127.0.0.1:0").unwrap());
            let addr = tcp_listener.local_addr().unwrap();
            let udp_socket = Arc::new(UdpSocket::bind(addr).unwrap());
            tx.send(addr).unwrap();
            let (clients, _) = setup_server(&addr.to_string()).unwrap();
            let udp_clients = Arc::new(Mutex::new(HashMap::new()));
            run_server(
                Arc::clone(&tcp_listener),
                Arc::clone(&udp_socket),
                clients,
                udp_clients,
                shutdown_rx,
                false,
            );
        });

        let server_addr = rx.recv().unwrap();
        thread::sleep(Duration::from_millis(100));

        let client1 = TcpStream::connect(server_addr).unwrap();
        let client2 = TcpStream::connect(server_addr).unwrap();

        let mut reader1 = BufReader::new(client1.try_clone().unwrap());
        let mut reader2 = BufReader::new(client2.try_clone().unwrap());
        let mut writer1 = BufWriter::new(client1.try_clone().unwrap());
        let mut writer2 = BufWriter::new(client2.try_clone().unwrap());

        // Register clients
        send_command(&mut writer1, &Command::Register("Alice".to_string())).unwrap();
        send_command(&mut writer2, &Command::Register("Bob".to_string())).unwrap();

        // Wait for registration confirmations
        assert!(matches!(
            receive_message(&mut reader1),
            Ok(Some(Err(ServerResponse::RegistrationSuccessful)))
        ));
        assert!(matches!(
            receive_message(&mut reader2),
            Ok(Some(Err(ServerResponse::RegistrationSuccessful)))
        ));

        // Send a file from Alice to Bob
        let test_content = b"Hello, this is a test file content.";
        send_command(
            &mut writer1,
            &Command::SendFile(
                "Bob".to_string(),
                "test.txt".to_string(),
                test_content.to_vec(),
            ),
        )
        .unwrap();

        // Check Bob's received file
        match receive_message(&mut reader2) {
            Ok(Some(Err(ServerResponse::FileTransferStarted(filename)))) => {
                assert_eq!(filename, "test.txt")
            }
            other => panic!("Unexpected response: {:?}", other),
        }

        match receive_message(&mut reader2) {
            Ok(Some(Ok(Command::SendFile(sender, filename, content)))) => {
                assert_eq!(sender, "Alice");
                assert_eq!(filename, "test.txt");
                assert_eq!(content, test_content);
            }
            other => panic!("Unexpected response: {:?}", other),
        }

        match receive_message(&mut reader2) {
            Ok(Some(Err(ServerResponse::FileTransferComplete(filename)))) => {
                assert_eq!(filename, "test.txt")
            }
            other => panic!("Unexpected response: {:?}", other),
        }

        // Disconnect clients
        send_command(&mut writer1, &Command::Disconnect).unwrap();
        send_command(&mut writer2, &Command::Disconnect).unwrap();

        // Stop the server
        shutdown_tx.send(()).unwrap();
        server_thread.join().unwrap();
    }

    #[test]
    fn test_udp_client_registration() {
        initialize();

        let server_addr = "127.0.0.1:0";
        let udp_socket = Arc::new(UdpSocket::bind(server_addr).unwrap());
        let server_addr = udp_socket.local_addr().unwrap();
        let udp_clients = Arc::new(Mutex::new(HashMap::new()));
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let server_thread = thread::spawn(move || {
            run_udp_server(
                (*udp_socket).try_clone().unwrap(),
                udp_clients,
                running_clone,
            );
        });

        // Give the server a moment to start
        thread::sleep(Duration::from_millis(100));

        // Create a UDP client
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_socket.connect(server_addr).unwrap();
        client_socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        // Send registration command
        let register_command = Command::Register("TestUser".to_string());
        let serialized = serde_json::to_string(&register_command).unwrap();
        client_socket.send(serialized.as_bytes()).unwrap();

        // Receive response
        let mut buf = [0; 1024];
        match client_socket.recv(&mut buf) {
            Ok(size) => {
                let response: ServerResponse = serde_json::from_slice(&buf[..size]).unwrap();
                assert!(matches!(response, ServerResponse::RegistrationSuccessful));
            }
            Err(e) => panic!("Failed to receive response: {}", e),
        }

        // Clean up
        running.store(false, Ordering::Relaxed);

        // Wait for the server thread to finish (with a timeout)
        let timeout = Duration::from_secs(5);
        let start = Instant::now();

        while start.elapsed() < timeout {
            if server_thread.is_finished() {
                server_thread.join().unwrap();
                println!("UDP server thread finished successfully");
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }

        panic!("UDP server thread did not finish within the timeout");
    }
}
