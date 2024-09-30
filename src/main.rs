use std::collections::HashMap;
use std::net::{TcpListener, UdpSocket};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::sync::Mutex;

use log::info;

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <server|client>", args[0]);
        return;
    }

    match args[1].as_str() {
        "server" => {
            info!("Starting server");
            let address = "127.0.0.1:8080";
            let tcp_listener =
                Arc::new(TcpListener::bind(address).expect("Failed to bind to address"));
            let udp_socket = Arc::new(UdpSocket::bind(address).expect("Failed to bind UDP socket"));
            let (clients, _) =
                net_chat::server::setup_server(address).expect("Failed to setup server");
            let udp_clients = Arc::new(Mutex::new(HashMap::new()));
            let (_, shutdown_rx) = channel();

            info!("Server is running on {}. Press Ctrl+C to stop.", address);
            net_chat::server::run_server(
                tcp_listener,
                udp_socket,
                clients,
                udp_clients,
                shutdown_rx,
                true,
            );
        }
        "client" => {
            let use_udp = args.get(3).map_or(false, |arg| arg == "udp");
            if let Err(e) = net_chat::client::start_client("127.0.0.1:8080", use_udp) {
                eprintln!("Client error: {}", e);
            }
        }
        _ => println!("Invalid argument. Use 'server' or 'client'"),
    }
}
