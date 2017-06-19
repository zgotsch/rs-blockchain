use serde;
use serde::ser::{Serialize, Serializer};
use serde::de::{Deserialize, Deserializer};

use byteorder;
use byteorder::ByteOrder;
use crypto::sha2::Sha256;
use crypto::digest::Digest;
use std::fmt;
use std::cmp::{PartialEq, Eq};

use time;


#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hash32Byte(pub [u8; 32]);
pub struct BlockData(pub [u8; 1024]);

impl PartialEq for BlockData {
    fn eq(&self, other: &BlockData) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }
        for i in 0..other.0.len() {
            if self.0[i] != other.0[i] {
                return false;
            }
        }
        return true;
    }
}
impl Eq for BlockData {}

impl Clone for BlockData {
    fn clone(&self) -> Self {
        let mut copy = [0; 1024];
        for i in 0..1024 {
            copy[i] = self.0[i];
        }
        return BlockData(copy);
    }
}

impl Serialize for BlockData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        serializer.serialize_bytes(&self.0)
        // let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        // for x in &self.0[..] {
        //     seq.serialize_element(x)?;
        // }
        // seq.end()
    }
}

impl<'de> Deserialize<'de> for BlockData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
       where D: Deserializer<'de>
    {
        struct BlockDataVisitor;

        impl<'de> serde::de::Visitor<'de> for BlockDataVisitor {
            type Value = BlockData;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("1024 bytes")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<BlockData, E>
                where E: serde::de::Error
            {
                let mut data = [0u8; 1024];
                data.copy_from_slice(v);
                Ok(BlockData(data))
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<BlockData, E>
                where E: serde::de::Error
            {
                let mut data = [0u8; 1024];
                for i in 0..1024 {
                    data[i] = v[i];
                }
                Ok(BlockData(data))
            }
        }

        deserializer.deserialize_bytes(BlockDataVisitor)
    }
}

impl fmt::Debug for BlockData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}...", self.0[0..10].iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""))
        // self.0[1..10].fmt(f)
    }
}

impl fmt::Debug for Hash32Byte {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let x : Vec<String> = self.0.iter().map(|b| format!("{:02x}", b)).collect();
        write!(f, "{}", x.join(""))
        // write!(f, "{}", encode(&self.0))
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Block {
    pub block_num: u64,
    pub previous_hash: Hash32Byte,
    pub timestamp: u64, // ns
    pub data: BlockData,
    pub hash: Hash32Byte
}

fn make_hash(block_num: u64, previous_hash: Hash32Byte, timestamp: u64, data: [u8; 1024]) -> Hash32Byte {
    let mut sha = Sha256::new();

    let mut buf = &mut [0; 8];
    byteorder::BigEndian::write_u64(buf, block_num);
    sha.input(buf);

    sha.input(&previous_hash.0);

    byteorder::BigEndian::write_u64(buf, timestamp);
    sha.input(buf);

    sha.input(&data);

    let mut output = [0; 32];
    sha.result(&mut output);

    // println!("{:?}", output);
    return Hash32Byte(output);
}

impl Block {
    pub fn new(past_block: &Block, data: [u8; 1024]) -> Block {
        let block_num = past_block.block_num + 1;
        let ts = time::precise_time_ns();
        Block{
            block_num: block_num,
            previous_hash: past_block.hash,
            timestamp: ts,
            data: BlockData(data),
            hash: make_hash(block_num, past_block.hash, ts, data)
        }
    }

    pub fn genesis() -> Block {
        Block {
            block_num: 0,
            previous_hash: Hash32Byte([0; 32]),
            timestamp: 0,
            data: BlockData([0; 1024]),
            hash: make_hash(0, Hash32Byte([0; 32]), 0, [0; 1024])
        }
    }
}

// impl<'de: 'a, 'a> Deserialize<'de> for &'a Block {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//         where D: Deserializer<'de>
//     {
//         Block::deserialize(deserializer).map(|b| &b)
//     }
// }

// #[derive(Debug, Serialize, Deserialize, Clone)]
// pub struct Blockchain(pub Vec<Block>);

// impl<'de: 'a, 'a> Deserialize<'de> for &'a Blockchain {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//         where D: Deserializer<'de>
//     {
//         Blockchain::deserialize(deserializer).map(|b| &b)
//     }
// }

pub fn check_chain<'a>(chain: &[Block]) -> bool {
    // check that the chain is unbroken, and the first block is genesis
    if let Some((first, rest)) = chain.split_first() {
        if *first != Block::genesis() {
            return false;
        }

        let (valid, _) = rest.iter().fold(
            (true, first), |acc, x|
                (acc.0 &&
                    x.previous_hash == acc.1.hash &&
                    x.block_num == acc.1.block_num + 1,
                x)
        );

        return valid;
    } else {
        return true;
    }
}
