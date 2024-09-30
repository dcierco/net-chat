use log::info;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::net::TcpListener;

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
            let listener = Arc::new(TcpListener::bind(address).expect("Failed to bind to address"));
            let (clients, _) = net_chat::server::setup_server(address).expect("Failed to setup server");
            let (_, shutdown_rx) = channel();

            info!("Server is running on {}. Press Ctrl+C to stop.", address);
            net_chat::server::run_server(listener, clients, shutdown_rx, true);
        }
        "client" => {
            info!("Starting client");
            if let Err(e) = net_chat::client::start_client("127.0.0.1:8080") {
                eprintln!("Client error: {}", e);
            }
        }
        _ => println!("Invalid argument. Use 'server' or 'client'"),
    }
}
