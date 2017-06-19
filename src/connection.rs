use std::net::TcpStream;
use std::io;
use std::io::{Read, Write};
use byteorder::{ByteOrder, NetworkEndian};
use bincode::{serialize, deserialize, Infinite};
use std::borrow::ToOwned;

use message::{ClientMessage};


pub struct Connection {
    next_message_size: Option<u64>,
    stream: TcpStream,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        Connection{
            next_message_size: None,
            stream: stream,
        }
    }

    pub fn write_message(&mut self, msg: &ClientMessage) -> io::Result<()> {
        let serialized = serialize(msg, Infinite).unwrap();

        // first write size big endian
        let mut size_buf = [0; 8];
        NetworkEndian::write_u64(&mut size_buf, serialized.len() as u64);
        self.stream.write(&size_buf)?;

        // now write the bytes
        self.stream.write(&serialized)?;

        Ok(())
    }

    pub fn read_message(&mut self) -> io::Result<ClientMessage> {
        // read the size of the payload
        let mut size_buf = [0; 8];
        self.stream.read_exact(&mut size_buf)?;
        let size = NetworkEndian::read_u64(&size_buf);

        let mut msg_buf = Vec::with_capacity(size as usize);
        msg_buf.resize(size as usize, 0);
        self.stream.read_exact(&mut msg_buf)?;

        let message: ClientMessage = deserialize(&msg_buf).unwrap();
        return Ok(message);
    }
}
