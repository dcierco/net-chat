use crate::common::{receive_message, send_command, send_file, Command, ServerResponse};
use log::{error, info};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn start_client(address: &str) -> std::io::Result<()> {
    info!("Connecting to server at {}", address);
    let stream = TcpStream::connect(address)?;
    let reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    print!("Enter your name: ");
    io::stdout().flush()?;
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim().to_string();

    send_command(&mut writer, &Command::Register(name.clone()))?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    // Start a thread to handle incoming messages
    let reader_clone = reader.get_ref().try_clone()?;
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader_clone);
        while running_clone.load(Ordering::SeqCst) {
            match receive_message(&mut reader) {
                Ok(Some(Ok(Command::Scream(msg)))) => println!("\nBroadcast: {}", msg),
                Ok(Some(Ok(Command::Whisper(from, msg)))) => {
                    println!("\nWhisper from {}: {}", from, msg)
                }
                Ok(Some(Err(ServerResponse::RegistrationSuccessful))) => {
                    println!("\nRegistration successful")
                }
                Ok(Some(Err(ServerResponse::Message(msg)))) => println!("\nServer: {}", msg),
                Ok(Some(Ok(Command::SendFile(sender, filename, content)))) => {
                    if let Err(e) = handle_received_file(&sender, &filename, &content) {
                        println!("\nFailed to save received file: {}", e);
                    }
                }
                Ok(Some(Err(ServerResponse::FileTransferStarted(filename)))) => {
                    println!("\nReceiving file: {}", filename)
                }
                Ok(Some(Err(ServerResponse::FileTransferComplete(filename)))) => {
                    println!("\nFile received: {}", filename)
                }
                Ok(Some(Err(ServerResponse::FileTransferFailed(error)))) => {
                    println!("\nFile transfer failed: {}", error)
                }
                Ok(Some(Err(ServerResponse::UserList(users)))) => {
                    println!("\nOnline users: {}", users.join(", "))
                }
                Ok(None) => {
                    println!("\nServer disconnected");
                    running_clone.store(false, Ordering::SeqCst);
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    error!("Error receiving message: {}", e);
                    running_clone.store(false, Ordering::SeqCst);
                    break;
                }
            }
            print!("Enter command: ");
            io::stdout().flush().unwrap();
        }
    });

    while running.load(Ordering::SeqCst) {
        print!("Enter command: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("quit") {
            send_command(&mut writer, &Command::Disconnect)?;
            break;
        }

        let parts: Vec<&str> = input.split('/').collect();
        if parts.len() >= 2 {
            match parts[1] {
                "Scream" => {
                    send_command(&mut writer, &Command::Scream(parts[2].to_string()))?;
                }
                "Whisper" => {
                    if parts.len() >= 4 {
                        send_command(
                            &mut writer,
                            &Command::Whisper(parts[2].to_string(), parts[3].to_string()),
                        )?;
                    } else {
                        println!("Invalid Whisper command. Use: /Whisper/recipient/message/");
                    }
                }
                "ListUsers" => {
                    send_command(&mut writer, &Command::ListUsers)?;
                }
                "SendFile" => {
                    if parts.len() >= 4 {
                        let recipient = parts[2];
                        let filename = parts[3];
                        send_file_command(&mut writer, recipient, filename)?;
                    } else {
                        println!("Invalid SendFile command. Use: /SendFile/recipient/filename");
                    }
                }
                _ => {
                    println!("Unknown command. Use /Scream/, /Whisper/, /ListUsers/, or /SendFile/")
                }
            }
        } else {
            println!("Invalid command format. Use: `/Command/Message/`, `/Whisper/<recipient>/Message/`, `/ListUsers/` or `/SendFile/<recipient>/<filename>/`");
        }
    }

    info!("Disconnecting from server");
    Ok(())
}

fn send_file_command(writer: &mut impl Write, recipient: &str, filename: &str) -> io::Result<()> {
    let path = Path::new(filename);
    if !path.exists() {
        println!("File not found: {}", filename);
        return Ok(());
    }

    let mut file = File::open(path)?;
    send_file(&mut file, recipient, filename, writer)?;
    println!("File sent: {}", filename);
    Ok(())
}

fn handle_received_file(sender: &str, filename: &str, content: &[u8]) -> io::Result<()> {
    let path = Path::new(filename);
    let display_name = path.file_name().unwrap().to_string_lossy();
    let mut file = File::create(path)?;
    file.write_all(content)?;
    println!("\nReceived file '{}' from {}", display_name, sender);
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::common::receive_command;
    use crate::initialize;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::Duration;
    use std::{fs, thread};

    #[test]
    fn test_client_registration_and_messaging() {
        initialize();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server_thread = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = BufWriter::new(stream);

            if let Ok(Some(command)) = receive_command(&mut reader) {
                assert!(matches!(command, Command::Register(name) if name == "TestUser"));
            }

            if let Ok(Some(command)) = receive_command(&mut reader) {
                assert!(matches!(command, Command::Scream(msg) if msg == "Hello, Server!"));
            }

            send_command(
                &mut writer,
                &Command::Scream("Message received".to_string()),
            )
            .unwrap();

            // Wait for client to disconnect
            assert!(matches!(
                receive_command(&mut reader),
                Ok(Some(Command::Disconnect))
            ));
        });

        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = BufWriter::new(stream);

        send_command(&mut writer, &Command::Register("TestUser".to_string())).unwrap();
        send_command(&mut writer, &Command::Scream("Hello, Server!".to_string())).unwrap();

        if let Ok(Some(received)) = receive_command(&mut reader) {
            assert!(matches!(received, Command::Scream(msg) if msg == "Message received"));
        } else {
            panic!("Did not receive expected message");
        }

        // Disconnect
        send_command(&mut writer, &Command::Disconnect).unwrap();

        // Close connection
        drop(writer);
        drop(reader);

        // Wait for the server thread to finish (with timeout)
        server_thread.join().unwrap();
    }

    #[test]
    fn test_handle_received_file() {
        // Create a temporary directory for our test files
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let sender = "Alice";
        let filename = "test_file.txt";
        let content = b"This is a test file content.";

        // Test file reception
        handle_received_file(sender, filename, content).unwrap();

        // Check if the file was created and has the correct content
        let file_path = PathBuf::from(filename);
        assert!(file_path.exists());
        let saved_content = fs::read(file_path).unwrap();
        assert_eq!(saved_content, content);

        // Clean up: change back to the original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_receive_file_command() {
        // Create a temporary directory for our test files
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let sender = "Bob";
        let filename = "received_file.txt";
        let content = b"Content of the received file.";

        let command = Command::SendFile(sender.to_string(), filename.to_string(), content.to_vec());

        // Simulate receiving a file through a command
        if let Command::SendFile(s, f, c) = command {
            handle_received_file(&s, &f, &c).unwrap();
        }

        // Check if the file was created and has the correct content
        let file_path = PathBuf::from(filename);
        assert!(file_path.exists());
        let saved_content = fs::read(file_path).unwrap();
        assert_eq!(saved_content, content);

        // Clean up: change back to the original directory
        std::env::set_current_dir(original_dir).unwrap();
    }
}
