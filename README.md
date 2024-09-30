# Net-Chat

Net-Chat is a simple client-server chat application written in Rust. It allows multiple clients to connect to a server, send messages to all connected clients (broadcast), send private messages (whisper), transfer files, and list online users.

## Features

- Client registration
- Broadcasting messages to all connected clients
- Sending private messages to specific clients
- File transfer between clients
- Listing online users

## Prerequisites

- Rust programming language (latest stable version)
- Cargo package manager

## Getting Started

1. Clone the repository:
   ```
   git clone https://github.com/your-username/net-chat.git
   cd net-chat
   ```

2. Build the project:
   ```
   cargo build --release
   ```

3. Run the server:
   ```
   cargo run --release -- server
   ```

4. In separate terminal windows, run multiple clients:
   ```
   cargo run --release -- client
   ```

## Usage

### Server

The server will start automatically and listen for incoming connections on `127.0.0.1:8080`.

### Client

After starting a client, you will be prompted to enter your name. Once registered, you can use the following commands:

- Broadcast a message: `/Scream/<message>/`
- Send a private message: `/Whisper/<recipient>/<message>/`
- Send a file: `/SendFile/<recipient>/<filename>/`
- List online users: `/ListUsers/`
- Disconnect: `quit`

## Running Tests

To run the tests, use the following command:

```
cargo test
```

## Project Structure

- `src/main.rs`: Entry point for the application
- `src/server.rs`: Server implementation
- `src/client.rs`: Client implementation
- `src/common.rs`: Shared data structures and utility functions
- `src/lib.rs`: Library functions and module declarations
- `tests/integration_tests.rs`: Integration tests

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
