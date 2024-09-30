use log::debug;
use serde::{ser::SerializeStruct, Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};

#[derive(Clone, PartialEq)]
pub enum Command {
    Register(String),
    Scream(String),
    Whisper(String, String),           // (recipient, message)
    SendFile(String, String, Vec<u8>), // (recipient, filename, file_content)
    ListUsers,
    Disconnect,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ServerResponse {
    RegistrationSuccessful,
    Message(String),
    UserList(Vec<String>),
    FileTransferStarted(String),  // filename
    FileTransferComplete(String), // filename
    FileTransferFailed(String),   // error message
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

pub fn send_file<R: Read>(
    reader: &mut R,
    recipient: &str,
    filename: &str,
    writer: &mut impl Write,
) -> io::Result<()> {
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
                Ok(command) => {
                    // Log the command without showing the file content
                    debug!("Received command: {:?}", command);
                    Ok(Some(command))
                }
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            }
        }
        Err(e) => Err(e),
    }
}

pub fn receive_message<R: BufRead>(
    reader: &mut R,
) -> io::Result<Option<Result<Command, ServerResponse>>> {
    let mut buffer = String::new();
    match reader.read_line(&mut buffer) {
        Ok(0) => Ok(None),
        Ok(_) => {
            if let Ok(command) = serde_json::from_str::<Command>(&buffer) {
                Ok(Some(Ok(command)))
            } else if let Ok(response) = serde_json::from_str::<ServerResponse>(&buffer) {
                Ok(Some(Err(response)))
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid message format",
                ))
            }
        }
        Err(e) => Err(e),
    }
}

// Custom Debug for Command
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Register(name) => write!(f, "Register({})", name),
            Command::Scream(msg) => write!(f, "Scream({})", msg),
            Command::Whisper(recipient, msg) => write!(f, "Whisper({}, {})", recipient, msg),
            Command::ListUsers => write!(f, "ListUsers"),
            Command::SendFile(recipient, filename, content) => {
                write!(
                    f,
                    "SendFile({}, {}, {} bytes)",
                    recipient,
                    filename,
                    content.len()
                )
            }
            Command::Disconnect => write!(f, "Disconnect"),
        }
    }
}

// Custom Serialize for Command
impl Serialize for Command {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("Command", 2)?;
        match self {
            Command::Register(name) => {
                state.serialize_field("type", "Register")?;
                state.serialize_field("name", name)?;
            }
            Command::Scream(msg) => {
                state.serialize_field("type", "Scream")?;
                state.serialize_field("message", msg)?;
            }
            Command::Whisper(recipient, msg) => {
                state.serialize_field("type", "Whisper")?;
                state.serialize_field("recipient", recipient)?;
                state.serialize_field("message", msg)?;
            }
            Command::SendFile(recipient, filename, content) => {
                state.serialize_field("type", "SendFile")?;
                state.serialize_field("recipient", recipient)?;
                state.serialize_field("filename", filename)?;
                state.serialize_field("content", content)?;
            }
            Command::ListUsers => {
                state.serialize_field("type", "ListUsers")?;
            }
            Command::Disconnect => {
                state.serialize_field("type", "Disconnect")?;
            }
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Type,
            Name,
            Message,
            Recipient,
            Filename,
            Content,
        }

        struct CommandVisitor;

        impl<'de> serde::de::Visitor<'de> for CommandVisitor {
            type Value = Command;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct Command")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Command, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let mut command_type = None;
                let mut name = None;
                let mut message = None;
                let mut recipient = None;
                let mut filename = None;
                let mut content = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Type => {
                            command_type = Some(map.next_value()?);
                        }
                        Field::Name => {
                            name = Some(map.next_value()?);
                        }
                        Field::Message => {
                            message = Some(map.next_value()?);
                        }
                        Field::Recipient => {
                            recipient = Some(map.next_value()?);
                        }
                        Field::Filename => {
                            filename = Some(map.next_value()?);
                        }
                        Field::Content => {
                            content = Some(map.next_value()?);
                        }
                    }
                }

                let command_type =
                    command_type.ok_or_else(|| serde::de::Error::missing_field("type"))?;
                match command_type {
                    "Register" => Ok(Command::Register(
                        name.ok_or_else(|| serde::de::Error::missing_field("name"))?,
                    )),
                    "Scream" => {
                        Ok(Command::Scream(message.ok_or_else(|| {
                            serde::de::Error::missing_field("message")
                        })?))
                    }
                    "Whisper" => Ok(Command::Whisper(
                        recipient.ok_or_else(|| serde::de::Error::missing_field("recipient"))?,
                        message.ok_or_else(|| serde::de::Error::missing_field("message"))?,
                    )),
                    "SendFile" => Ok(Command::SendFile(
                        recipient.ok_or_else(|| serde::de::Error::missing_field("recipient"))?,
                        filename.ok_or_else(|| serde::de::Error::missing_field("filename"))?,
                        content.ok_or_else(|| serde::de::Error::missing_field("content"))?,
                    )),
                    "ListUsers" => Ok(Command::ListUsers),
                    "Disconnect" => Ok(Command::Disconnect),
                    _ => Err(serde::de::Error::unknown_variant(
                        &command_type,
                        &[
                            "Register",
                            "Scream",
                            "Whisper",
                            "SendFile",
                            "ListUsers",
                            "Disconnect",
                        ],
                    )),
                }
            }
        }

        deserializer.deserialize_struct(
            "Command",
            &[
                "type",
                "name",
                "message",
                "recipient",
                "filename",
                "content",
            ],
            CommandVisitor,
        )
    }
}
