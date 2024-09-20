use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{self, AsyncWriteExt, AsyncReadExt};
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;

async fn client_main(server_address: &str, client_address: &str) -> io::Result<()> {
    let mut tcp_stream = TcpStream::connect(server_address).await?;
    let udp_socket = UdpSocket::bind(client_address).await?;
    udp_socket.connect(server_address).await?;

    loop {
        match get_user_input().await {
            Ok(Command::Register(username, password)) => {
                let command = format!("REGISTER|{}|{}", username, password);
                tcp_stream.write_all(command.as_bytes()).await?;
            }
            Ok(Command::Authenticate(username, password)) => {
                let command = format!("AUTH|{}|{}", username, password);
                tcp_stream.write_all(command.as_bytes()).await?;

                let mut buffer = [0; 1024];
                let n = tcp_stream.read(&mut buffer).await?;
                println!("Server: {}", String::from_utf8_lossy(&buffer[..n]));
            }
            Ok(Command::SendMessage(to, message)) => {
                let command = format!("MESSAGE|{}|{}", to, message);
                tcp_stream.write_all(command.as_bytes()).await?;
            }
            Ok(Command::ListUsers) => {
                tcp_stream.write_all(b"LIST_USERS").await?;
            }
            Ok(Command::SendFile(from, to, file_path)) => {
                send_file(from, to, file_path, &udp_socket).await?;
            }
            Ok(Command::Logout(username)) => {
                let command = format!("LOGOUT|{}", username);
                tcp_stream.write_all(command.as_bytes()).await?;
                break;
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}

async fn get_user_input() -> Result<Command, &'static str> {
    println!("Enter command (register, auth, message, list, file, logout): ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();

    if input.starts_with("register") {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() != 3 {
            return Err("Usage: register <username> <password>");
        }
        Ok(Command::Register(parts[1].to_string(), parts[2].to_string()))
    } else if input.starts_with("auth") {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() != 3 {
            return Err("Usage: auth <username> <password>");
        }
        Ok(Command::Authenticate(parts[1].to_string(), parts[2].to_string()))
    } else if input.starts_with("message") {
        let parts: Vec<&str> = input.splitn(3, ' ').collect();
        if parts.len() != 3 {
            return Err("Usage: message <to> <message>");
        }
        Ok(Command::SendMessage(parts[1].to_string(), parts[2].to_string()))
    } else if input == "list" {
        Ok(Command::ListUsers)
    } else if input.starts_with("file") {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() != 4 {
            return Err("Usage: file <from> <to> <file_path>");
        }
        Ok(Command::SendFile(parts[1].to_string(), parts[2].to_string(), parts[3].to_string()))
    } else if input.starts_with("logout") {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() != 2 {
            return Err("Usage: logout <username>");
        }
        Ok(Command::Logout(parts[1].to_string()))
    } else {
        Err("Unknown command")
    }
}

async fn send_file(from: String, to: String, file_path: String, udp_socket: &UdpSocket) -> io::Result<()> {
    let file = File::open(&file_path)?;
    let size = file.metadata()?.len() as usize; // Get total file size

    let mut reader = BufReader::new(file);
    let mut buffer = [0; 1024];
    let mut packet_number = 0;

    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 { break; }
        let packet = format!("{}|{}|{}|{}|{}", from, to, file_path, packet_number, size).into_bytes();
        udp_socket.send(&[packet, buffer[..n].to_vec()].concat()).await?;
        packet_number += 1;
    }
    Ok(())
}

#[derive(Debug)]
enum Command {
    Register(String, String),
    Authenticate(String, String),
    SendMessage(String, String),
    SendFile(String, String, String),
    ListUsers,
    Logout(String),
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