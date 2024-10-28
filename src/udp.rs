use crate::common::{Command, ServerResponse};
use log::{debug, error, info};
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type UdpClientMap = Arc<Mutex<HashMap<String, std::net::SocketAddr>>>;

pub fn run_udp_server(socket: UdpSocket, clients: UdpClientMap, running: Arc<AtomicBool>) {
    info!("Starting UDP server on {}", socket.local_addr().unwrap());

    socket
        .set_nonblocking(true)
        .expect("Could not set UDP socket to non-blocking");

    let mut buf = [0; 65507]; // Maximum UDP datagram size
    while running.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((size, src)) => match String::from_utf8(buf[..size].to_vec()) {
                Ok(received) => {
                    debug!("Received UDP message: {} from {}", received, src);
                    match serde_json::from_str::<Command>(&received) {
                        Ok(command) => {
                            handle_udp_command(&command, src, &socket, &clients);
                        }
                        Err(e) => {
                            error!("Failed to parse UDP command: {}", e);
                            send_udp_response(
                                &socket,
                                &ServerResponse::Message("Invalid command format".to_string()),
                                src,
                            );
                        }
                    }
                }
                Err(e) => {
                    error!("Invalid UTF-8 in UDP message: {}", e);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => {
                error!("Error receiving UDP message: {}", e);
            }
        }
    }
    info!("UDP server shutting down");
}

fn handle_udp_command(
    command: &Command,
    src: std::net::SocketAddr,
    socket: &UdpSocket,
    clients: &UdpClientMap,
) {
    match command {
        Command::Register(name) => {
            let mut clients = clients.lock().unwrap();
            // Check if name is already taken
            if clients.values().any(|&addr| addr == src) {
                send_udp_response(
                    socket,
                    &ServerResponse::Message("Already registered".to_string()),
                    src,
                );
                return;
            }
            if clients.contains_key(name) {
                send_udp_response(
                    socket,
                    &ServerResponse::Message("Name already taken".to_string()),
                    src,
                );
                return;
            }

            clients.insert(name.clone(), src);
            send_udp_response(socket, &ServerResponse::RegistrationSuccessful, src);
            broadcast_message(
                socket,
                &clients,
                &format!("User {} has joined the chat", name),
                None,
            );
            info!("Registered UDP client: {} at {}", name, src);
        }

        Command::Scream(message) => {
            let clients = clients.lock().unwrap();
            if let Some((sender_name, _)) = find_client_by_addr(&clients, src) {
                let message_to_broadcast = format!("{}: {}", sender_name, message);
                broadcast_message(socket, &clients, &message_to_broadcast, Some(src));
                info!("Broadcast from {}: {}", sender_name, message);
            } else {
                send_udp_response(
                    socket,
                    &ServerResponse::Message("Not registered".to_string()),
                    src,
                );
            }
        }

        Command::Whisper(recipient, message) => {
            let clients = clients.lock().unwrap();
            if let Some((sender_name, _)) = find_client_by_addr(&clients, src) {
                if let Some(&recipient_addr) = clients.get(recipient) {
                    let whisper_command = Command::Whisper(sender_name.clone(), message.clone());
                    send_udp_command(socket, &whisper_command, recipient_addr);
                    info!("Whisper from {} to {}: {}", sender_name, recipient, message);
                } else {
                    send_udp_response(
                        socket,
                        &ServerResponse::Message(format!("User {} not found", recipient)),
                        src,
                    );
                }
            } else {
                send_udp_response(
                    socket,
                    &ServerResponse::Message("Not registered".to_string()),
                    src,
                );
            }
        }

        Command::ListUsers => {
            let clients = clients.lock().unwrap();
            if find_client_by_addr(&clients, src).is_some() {
                let users: Vec<String> = clients.keys().cloned().collect();
                send_udp_response(socket, &ServerResponse::UserList(users), src);
            } else {
                send_udp_response(
                    socket,
                    &ServerResponse::Message("Not registered".to_string()),
                    src,
                );
            }
        }

        Command::SendFile(recipient, filename, content) => {
            let clients = clients.lock().unwrap();
            if let Some((sender_name, _)) = find_client_by_addr(&clients, src) {
                if let Some(&recipient_addr) = clients.get(recipient) {
                    // Notify recipient about incoming file
                    send_udp_response(
                        socket,
                        &ServerResponse::FileTransferStarted(filename.clone()),
                        recipient_addr,
                    );

                    // Send the file
                    let file_command =
                        Command::SendFile(sender_name.clone(), filename.clone(), content.clone());
                    send_udp_command(socket, &file_command, recipient_addr);

                    // Confirm completion
                    send_udp_response(
                        socket,
                        &ServerResponse::FileTransferComplete(filename.clone()),
                        recipient_addr,
                    );
                    info!(
                        "File '{}' transferred from {} to {}",
                        filename, sender_name, recipient
                    );
                } else {
                    send_udp_response(
                        socket,
                        &ServerResponse::FileTransferFailed(format!(
                            "User {} not found",
                            recipient
                        )),
                        src,
                    );
                }
            } else {
                send_udp_response(
                    socket,
                    &ServerResponse::Message("Not registered".to_string()),
                    src,
                );
            }
        }

        Command::Disconnect => {
            let mut clients = clients.lock().unwrap();
            if let Some((name, _)) = find_client_by_addr(&clients, src) {
                clients.remove(&name);
                broadcast_message(
                    socket,
                    &clients,
                    &format!("User {} has left the chat", name),
                    None,
                );
                info!("Client {} disconnected", name);
            }
        }
    }
}

fn find_client_by_addr(
    clients: &HashMap<String, std::net::SocketAddr>,
    addr: std::net::SocketAddr,
) -> Option<(String, std::net::SocketAddr)> {
    clients
        .iter()
        .find(|(_, &client_addr)| client_addr == addr)
        .map(|(name, &addr)| (name.clone(), addr))
}

fn broadcast_message(
    socket: &UdpSocket,
    clients: &HashMap<String, std::net::SocketAddr>,
    message: &str,
    exclude_addr: Option<std::net::SocketAddr>,
) {
    let command = Command::Scream(message.to_string());
    for &addr in clients.values() {
        if Some(addr) != exclude_addr {
            send_udp_command(socket, &command, addr);
        }
    }
}

fn send_udp_response(socket: &UdpSocket, response: &ServerResponse, addr: std::net::SocketAddr) {
    let serialized = serde_json::to_string(response).unwrap();
    if let Err(e) = socket.send_to(serialized.as_bytes(), addr) {
        error!("Error sending UDP response: {}", e);
    }
}

fn send_udp_command(socket: &UdpSocket, command: &Command, addr: std::net::SocketAddr) {
    let serialized = serde_json::to_string(command).unwrap();
    if let Err(e) = socket.send_to(serialized.as_bytes(), addr) {
        error!("Error sending UDP command: {}", e);
    }
}
