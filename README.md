# net-chat

## Overview

`net-chat` is a Rust-based chat application designed for efficient and secure communication. It supports both text messaging and file transfers using TCP and UDP protocols. The application leverages Rust's asynchronous capabilities provided by the `tokio` library to ensure high performance and scalability.

## Features

- **User Registration and Authentication**: Securely register and authenticate users.
- **Text Messaging**: Reliable text messaging using TCP.
- **File Transfers**: Fast file transfers using UDP with custom reliability mechanisms.
- **Concurrency**: Efficient handling of multiple clients using Rust's async features.

## System Architecture

### Client Module

- Interprets user commands.
- Communicates with the server.
- Displays messages to the user.

### Server Module

- Manages client connections and user registration.
- Facilitates 1:1 communication.
- Supports both text messages and file transfers via TCP and UDP.

### Communication Protocols

- **TCP/IPv4**: Used for reliable communication, especially for user registration, authentication, and text messaging.
- **UDP/IPv4**: Used for faster, smaller data transfers (e.g., file transfers).

## Getting Started

### Prerequisites

- Rust and Cargo installed. You can install Rust from [rust-lang.org](https://www.rust-lang.org/).

### Installation

1. Clone the repository:
    ```sh
    git clone https://github.com/yourusername/net-chat.git
    cd net-chat
    ```

2. Build the project:
    ```sh
    cargo build --release
    ```

### Running the Server

To start the server, run:
```sh
cargo run --bin server
```

The server will start listening on 127.0.0.1:8080 for TCP connections and 127.0.0.1:8081 for UDP connections.

### Running the Client

To start the client, run:
```sh
cargo run --bin client <server_address> <client_address>
```
Replace <server_address> with the server's address (e.g., 127.0.0.1:8080) and <client_address> with the client's address (e.g., 127.0.0.1:8082).

#### Usage

Once the client is running, you can enter the following commands:

- **Register**: Register a new user.
    ```
    register <username> <password>
    ```

- **Authenticate**: Authenticate an existing user.
    ```
    auth <username> <password>
    ```

- **Send Message**: Send a text message to another user.
    ```
    message <to> <message>
    ```

- **List Users**: List all online users.
    ```
    list
    ```

- **Send File**: Send a file to another user.
    ```
    file <from> <to> <file_path>
    ```

- **Logout**: Logout the current user.
    ```
    logout <username>
    ```

## Protocol Overview

### Message Types and Structure

1. **REGISTER**:
    - **Purpose**: User registration.
    - **Structure**: `REGISTER|<username>|<hashed_password>`
    - **Response**: `REGISTER|SUCCESS` or `REGISTER|FAILURE|<reason>`

2. **AUTH**:
    - **Purpose**: User authentication.
    - **Structure**: `AUTH|<username>|<hashed_password>`
    - **Response**: `AUTH|SUCCESS|<session_token>` or `AUTH|FAILURE|<reason>`

3. **MESSAGE**:
    - **Purpose**: Sending text messages.
    - **Structure**: `MESSAGE|<from>|<to>|<content>`
    - **Response**: `MESSAGE|DELIVERED` or `MESSAGE|FAILURE|<reason>`

4. **FILE_TRANSFER**:
    - **Purpose**: Transfer files.
    - **Structure**: `FILE_TRANSFER|<from>|<to>|<filename>|<file_size>|<file_data>`
    - **Response**: `FILE_TRANSFER|DELIVERED` or `FILE_TRANSFER|FAILURE|<reason>`

5. **LIST_USERS**:
    - **Purpose**: List online users.
    - **Structure**: `LIST_USERS`
    - **Response**: `LIST_USERS|<user1>,<user2>,...`

6. **LOGOUT**:
    - **Purpose**: Logout a user.
    - **Structure**: `LOGOUT|<username>`
    - **Response**: `LOGOUT|SUCCESS` or `LOGOUT|FAILURE|<reason>`

## Error Handling

- **Malformed Messages**: Response with `ERROR|<reason>`.
- **Registration Errors**: Example: `REGISTER|FAILURE|Username already taken.`
- **Delivery Errors**: Example: `MESSAGE|FAILURE|User not online.`
- **File Transfer Errors**: Retry mechanism for dropped packets over UDP.

## Performance and Testing

### Test Setup

- **Tools Used**:
  - **Criterion.rs**: For benchmarking Rust code.
  - **Network Simulators**: Tools like `tc` and `netem` for simulating various network conditions.
  - **Monitoring**: Custom scripts to log metrics like delivery time, throughput, and server load.

### Test Cases

1. **Low Latency Network**: Simulate ideal conditions with minimal delay (e.g., 10ms).
2. **High Latency Network**: Introduce significant delay (e.g., 150ms).
3. **Packet Loss Simulation**: Simulate packet loss rates of 5% and 10%.

### Metrics Measured

- **Delivery Time**: Time taken for a message or file to be fully delivered.
- **Throughput**: Number of messages/files successfully transferred per unit time.
- **Latency**: Time delay experienced in message delivery.
- **Server Load**: CPU and memory usage on the server during testing.

## Conclusion

The `net-chat` application demonstrates the effective use of Rust's async capabilities and memory safety features to build a robust and efficient chat system. By balancing the reliability of TCP with the speed of UDP, the application ensures high performance and data integrity across varied network conditions.

## License

This project is licensed under the MIT License - see the LICENSE file for details.