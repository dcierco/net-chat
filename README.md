# Net-Chat

Net-Chat is a client-server chat application written in Rust. It allows multiple clients to connect to a server, send messages to all connected clients (broadcast), send private messages (whisper), transfer files, and list online users.

## Features

- Client registration
- Broadcasting messages to all connected clients
- Sending private messages to specific clients
- File transfer between clients
- Listing online users
- Support for both TCP and UDP connections

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
   cargo run --release -- client <server_address> [udp]
   ```
   Replace `<server_address>` with the server's IP address and port (e.g., 192.168.1.100:8080).
   Add `udp` at the end to use UDP instead of TCP.

## Usage

### Server

The server will start automatically and listen for incoming connections on `0.0.0.0:8080`.

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
- `src/udp.rs`: UDP-specific functionality
- `src/lib.rs`: Library functions and module declarations
- `tests/integration_tests.rs`: Integration tests

## Manual Testing Files

For manual testing of file transfer functionality, the following files are provided:

- `test_file.txt`: A small text file with some content (1.2 KB)
- `medium_file.txt`: A medium-sized text file with more content (1.1 MB)
- `big_file.txt`: A larger text file with a lot of content (2.1 MB)

These files can be used to test file transfer capabilities and compare performance between TCP and UDP implementations.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

---

## Análise de Desempenho

Nomes: Daniel Cierco e Josué da Silva Nascimento

1) Ao executar o Wireshark para monitorar o tráfego UDP, seria possível identificar os pacotes UDP sendo enviados para os servidores. As portas de origem seriam portas efêmeras alocadas pelo sistema operacional (geralmente acima de 49152), e a porta de destino seria a porta em que o servidor UDP está escutando (neste caso, 8080).

2) Em termos de volume de tráfego, a aplicação com socket UDP tende a gerar menos overhead de rede comparada à TCP, pois não há estabelecimento de conexão nem confirmações de recebimento.

3) Em termos de desempenho, a aplicação UDP pode ser mais rápida para mensagens pequenas devido à ausência de handshake e controle de fluxo. No entanto, para transferências de arquivos grandes ou em redes não confiáveis, TCP pode oferecer melhor desempenho devido à sua confiabilidade.

4) Para um arquivo de 1200 bytes, a transmissão via TCP e UDP seria similar em condições de rede ideais. TCP garantiria a entrega, enquanto UDP seria ligeiramente mais rápido, mas sem garantias.

5) Para um arquivo de 2000 bytes, a diferença seria semelhante à situação anterior, com TCP oferecendo confiabilidade e UDP oferecendo velocidade potencialmente maior, mas sem garantias de entrega ou ordem.

6) Com perda de pacotes configurada:
   a. TCP retransmitiria pacotes perdidos, mantendo a integridade dos dados, mas aumentando o tráfego. UDP não faria retransmissões, resultando em perda de dados, mas mantendo o volume de tráfego constante.

7) Com latência variável:
   a. TCP ajustaria seu timeout de retransmissão baseado na latência percebida, potencialmente causando retransmissões em casos de latência alta. UDP não seria afetado diretamente pela latência variável, mas a aplicação poderia experimentar atrasos ou desordem na entrega de mensagens.
