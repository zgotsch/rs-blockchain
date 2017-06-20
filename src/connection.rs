use std::net::{TcpStream, SocketAddr};
use std::io;
use std::io::{Read, Write};
use byteorder::{ByteOrder, NetworkEndian};
use bincode::{serialize, deserialize, Infinite};
use serde::ser::Serialize;
use serde::de::DeserializeOwned;


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

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    pub fn write_message<M>(&mut self, msg: &M) -> io::Result<()> where M: Serialize  {
        let serialized = serialize(msg, Infinite).unwrap();

        // first write size big endian
        let mut size_buf = [0; 8];
        NetworkEndian::write_u64(&mut size_buf, serialized.len() as u64);
        self.stream.write(&size_buf)?;

        // now write the bytes
        self.stream.write(&serialized)?;

        Ok(())
    }

    pub fn read_message<M>(&mut self) -> io::Result<Option<M>> where M: DeserializeOwned {
        // read the size of the payload
        let mut size_buf = [0; 8];
        let bytes_read = self.stream.read(&mut size_buf)?;

        if bytes_read == 0 {
            Ok(None)
        } else if bytes_read != 8 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid message length"));
        } else {
            let size = NetworkEndian::read_u64(&size_buf);

            let mut msg_buf = Vec::with_capacity(size as usize);
            msg_buf.resize(size as usize, 0);
            self.stream.read_exact(&mut msg_buf)?;

            let message: M = deserialize(&msg_buf).unwrap();
            return Ok(Some(message));
        }
    }
}
