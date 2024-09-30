use net_chat::common::{receive_command, send_command, Command};
use net_chat::{initialize, server};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
        let (clients, _) = server::setup_server(&addr.to_string()).unwrap();
        let udp_clients = Arc::new(Mutex::new(HashMap::new()));
        server::run_server(
            tcp_listener,
            udp_socket,
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

    client1
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client2
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let mut reader1 = BufReader::new(client1.try_clone().unwrap());
    let mut reader2 = BufReader::new(client2.try_clone().unwrap());
    let mut writer1 = BufWriter::new(client1);
    let mut writer2 = BufWriter::new(client2);

    send_command(&mut writer1, &Command::Register("Alice".to_string())).unwrap();
    send_command(&mut writer2, &Command::Register("Bob".to_string())).unwrap();

    // Wait for registration confirmation
    let _ = receive_command(&mut reader1);
    let _ = receive_command(&mut reader2);

    // Alice sends a message
    send_command(&mut writer1, &Command::Scream("Hello, Bob!".to_string())).unwrap();

    // Bob should receive Alice's message
    let received = receive_command(&mut reader2);
    assert!(matches!(received, Ok(Some(Command::Scream(msg))) if msg == "Alice: Hello, Bob!"));

    // Bob sends a reply
    send_command(&mut writer2, &Command::Scream("Hi, Alice!".to_string())).unwrap();

    // Alice should receive Bob's message
    let received = receive_command(&mut reader1);
    assert!(matches!(received, Ok(Some(Command::Scream(msg))) if msg == "Bob: Hi, Alice!"));

    // Disconnect clients
    send_command(&mut writer1, &Command::Disconnect).unwrap();
    send_command(&mut writer2, &Command::Disconnect).unwrap();

    // Close connections
    drop(writer1);
    drop(writer2);

    // Stop the server
    println!("Shutting down the server");
    shutdown_tx.send(()).unwrap();

    // Wait for the server thread to finish
    server_thread.join().unwrap();
    println!("Server shut down successfully");
}
