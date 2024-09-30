use serde::{Serialize, Deserialize};
use std::io::{self, BufRead, Write, Read};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Command {
    Register(String),
    Scream(String),
    Whisper(String, String), // (recipient, message)
    SendFile(String, String, Vec<u8>), // (recipient, filename, file_content)
    ListUsers,
    Disconnect,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ServerResponse {
    RegistrationSuccessful,
    Message(String),
    UserList(Vec<String>),
    FileTransferStarted(String), // filename
    FileTransferComplete(String), // filename
    FileTransferFailed(String), // error message
}

// Update send_command and receive_command functions to handle ServerResponse
pub fn send_command<W: Write>(writer: &mut W, command: &Command) -> io::Result<()> {
    let serialized = serde_json::to_string(command)?;
    writer.write_all(serialized.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn send_server_response<W: Write>(writer: &mut W, response: &ServerResponse) -> io::Result<()> {
    let serialized = serde_json::to_string(response)?;
    writer.write_all(serialized.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn send_file<R: Read>(reader: &mut R, recipient: &str, filename: &str, writer: &mut impl Write) -> io::Result<()> {
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    let command = Command::SendFile(recipient.to_string(), filename.to_string(), buffer);
    send_command(writer, &command)
}

pub fn receive_command<R: BufRead>(reader: &mut R) -> io::Result<Option<Command>> {
    let mut buffer = String::new();
    match reader.read_line(&mut buffer) {
        Ok(0) => Ok(None),
        Ok(_) => {
            match serde_json::from_str(&buffer.trim()) {
                Ok(command) => Ok(Some(command)),
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            }
        }
        Err(e) => Err(e),
    }
}

pub fn receive_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Result<Command, ServerResponse>>> {
    let mut buffer = String::new();
    match reader.read_line(&mut buffer) {
        Ok(0) => Ok(None),
        Ok(_) => {
            if let Ok(command) = serde_json::from_str::<Command>(&buffer) {
                Ok(Some(Ok(command)))
            } else if let Ok(response) = serde_json::from_str::<ServerResponse>(&buffer) {
                Ok(Some(Err(response)))
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid message format"))
            }
        }
        Err(e) => Err(e),
    }
}
