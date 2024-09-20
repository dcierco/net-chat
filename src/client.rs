use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{self, AsyncWriteExt};
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;

#[derive(Debug)]
enum Command {
    SendMessage(String),
    SendFile(String),
}

async fn client_main(server_address: &str, client_address: &str) -> io::Result<()> {
    let mut tcp_stream = TcpStream::connect(server_address).await?;
    let udp_socket = UdpSocket::bind(client_address).await?;
    udp_socket.connect(server_address).await?;

    loop {
        match get_user_input().await {
            Command::SendMessage(msg) => {
                tcp_stream.write_all(msg.as_bytes()).await?;
            }
            Command::SendFile(file_path) => {
                send_file(file_path, &udp_socket).await?;
            }
        }
    }
}

async fn get_user_input() -> Command {
    println!("Enter command (message or file): ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();

    if input.starts_with("message") {
        let msg = input.strip_prefix("message ").unwrap_or("").to_string();
        Command::SendMessage(msg)
    } else if input.starts_with("file") {
        let file_path = input.strip_prefix("file ").unwrap_or("").to_string();
        Command::SendFile(file_path)
    } else {
        panic!("Unknown command");
    }
}

async fn send_file(file_path: String, udp_socket: &UdpSocket) -> io::Result<()> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0; 1024];
    let mut packet_number = 0;

    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 { break; }
        let packet = format!("{}|{}|", packet_number, n).into_bytes();
        udp_socket.send(&[packet, buffer[..n].to_vec()].concat()).await?;
        packet_number += 1;
    }
    Ok(())
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