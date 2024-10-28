use std::collections::HashMap;
use std::net::{TcpListener, UdpSocket};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::sync::Mutex;

use log::info;

const TCP_PORT: u16 = 8080;
const UDP_PORT: u16 = 8081;
const SERVER_HOST: &str = "0.0.0.0";

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage:");
        println!("  Server: {} server", args[0]);
        println!("  Client: {} client <server_address> [udp]", args[0]);
        return;
    }

    match args[1].as_str() {
        "server" => {
            info!("Starting server");
            let tcp_address = format!("{}:{}", SERVER_HOST, TCP_PORT);
            let udp_address = format!("{}:{}", SERVER_HOST, UDP_PORT);

            let tcp_listener =
                Arc::new(TcpListener::bind(&tcp_address).expect("Failed to bind TCP address"));
            let udp_socket =
                Arc::new(UdpSocket::bind(&udp_address).expect("Failed to bind UDP address"));

            let (clients, _) =
                net_chat::server::setup_server(&tcp_address).expect("Failed to setup server");
            let udp_clients = Arc::new(Mutex::new(HashMap::new()));
            let (_, shutdown_rx) = channel();

            info!(
                "Server is running on TCP port {} and UDP port {}. Press Ctrl+C to stop.",
                TCP_PORT, UDP_PORT
            );
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
            if args.len() < 3 {
                println!("Usage: {} client <server_address> [udp]", args[0]);
                return;
            }

            let server_host = &args[2];
            let use_udp = args.get(3).map_or(false, |arg| arg == "udp");

            // Use the appropriate port based on protocol
            let port = if use_udp { UDP_PORT } else { TCP_PORT };
            let address = format!("{}:{}", server_host, port);

            if let Err(e) = net_chat::client::start_client(&address, use_udp) {
                eprintln!("Client error: {}", e);
            }
        }
        _ => println!("Invalid argument. Use 'server' or 'client'"),
    }
}
