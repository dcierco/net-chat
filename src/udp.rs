use crate::common::{Command, ServerResponse};
use log::{debug, error, info};
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub fn run_udp_server(
    socket: UdpSocket,
    clients: Arc<Mutex<HashMap<String, std::net::SocketAddr>>>,
    running: Arc<AtomicBool>,
) {
    socket
        .set_nonblocking(true)
        .expect("Could not set UDP socket to non-blocking");

    let mut buf = [0; 1024];
    while running.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((size, src)) => {
                let received = String::from_utf8_lossy(&buf[..size]);
                debug!("Received UDP message: {} from {}", received, src);

                // Parse the command and handle it
                if let Ok(command) = serde_json::from_str::<Command>(&received) {
                    handle_udp_command(command, src, &socket, &clients);
                } else {
                    error!("Invalid UDP command received from {}", src);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, sleep for a short while
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => {
                error!("Error receiving UDP message: {}", e);
                break;
            }
        }
    }
    info!("UDP server shutting down");
}

fn handle_udp_command(
    command: Command,
    src: std::net::SocketAddr,
    socket: &UdpSocket,
    clients: &Arc<Mutex<HashMap<String, std::net::SocketAddr>>>,
) {
    match command {
        Command::Register(name) => {
            clients.lock().unwrap().insert(name.clone(), src);
            let response = ServerResponse::RegistrationSuccessful;
            send_udp_response(socket, &response, src);
            info!("Registered UDP client: {} at {}", name, src);
        }
        Command::Scream(message) => {
            let broadcast_message = format!("{}: {}", src, message);
            for (_, addr) in clients.lock().unwrap().iter() {
                if *addr != src {
                    send_udp_command(socket, &Command::Scream(broadcast_message.clone()), *addr);
                }
            }
        }
        // Implement other commands (Whisper, SendFile, ListUsers, Disconnect) similarly
        _ => {
            error!("Unimplemented UDP command: {:?}", command);
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
