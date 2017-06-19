extern crate serde;

#[macro_use]
extern crate serde_derive;
extern crate crypto;
extern crate byteorder;
extern crate bincode;
extern crate base64;
extern crate time;
extern crate getopts;

use std::io::prelude::*;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::fs::OpenOptions;
use std::net::{TcpStream, TcpListener};
use std::net::ToSocketAddrs;
use std::time::Duration;
use std::thread;
use std::sync::Arc;
use std::sync::Mutex;

use bincode::{serialize, deserialize, Infinite};

mod block;
use block::{Block};
use block::check_chain;

mod message;
use message::{ClientMessage, NameServerMessage};

mod connection;
use connection::Connection;


fn print_usage(program: &str, opts: getopts::Options) {
    let brief = format!("Usage: {} OPTIONS", program);
    print!("{}", opts.usage(&brief));
}

fn handle_peer(stream: TcpStream, chain: Arc<Mutex<Vec<Block>>>) {
    let mut connection = Connection::new(stream);
    loop {
        let incoming = connection.read_message();
        match incoming {
            Ok(msg) => match msg {
                ClientMessage::QueryChain => {
                    let chain = chain.lock().unwrap();
                    connection.write_message(&ClientMessage::Chain(chain.clone()));
                },
                _ => {},
            },
            Err(_) => break
        }
    }
}

enum ReplCommand {
    NewBlock,
    ShowChain,
    Exit,
    // ListPeers,
}

fn parse(input: String) -> Result<ReplCommand, String> {
    match input.split_whitespace().next() {
        Some("block") => Ok(ReplCommand::NewBlock),
        Some("chain") => Ok(ReplCommand::ShowChain),
        Some("exit") => Ok(ReplCommand::Exit),
        Some(_) => Err("Unrecognized input".to_string()),
        None => Err("no input".to_string()),
    }
}


fn main() {
    // flags
    let args: Vec<String> = std::env::args().collect();
    let mut opts = getopts::Options::new();
    opts.reqopt("n", "nameserver", "nameserver address", "ADDR")
        .optopt("c", "chainfile", "chainfile location", "FILE")
        .optflag("h", "help", "show this message");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error");
            print_usage(&args[0], opts);
            std::process::exit(1);
        }
    };

    if matches.opt_present("h") {
        print_usage(&args[0], opts);
        return;
    }
    let chainfile_name = matches.opt_str("c").unwrap_or("my.chain".to_string());
    let nameserver_str = matches.opt_str("n").expect("Missing nameserver address.");

    // connect to nameserver
    let mut nameserver_stream = TcpStream::connect(nameserver_str).expect("Couldn't connet to nameserver");
    nameserver_stream.set_read_timeout(Some(Duration::from_millis(500))).expect("Couldn't set read timeout");

    // load your chain
    let mut chainfile = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(chainfile_name)
            .unwrap();
    let mut chain_serialized = Vec::new();
    chainfile.read_to_end(&mut chain_serialized).expect("Couldn't read chain file");
    let chain: Arc<Mutex<Vec<Block>>> = Arc::new(Mutex::new(if chain_serialized.len() == 0 {
        vec![Block::genesis()]
    } else {
        deserialize(&chain_serialized).unwrap()
    }));

    // listen for peers
    let listener = Arc::new(TcpListener::bind(("::", 0)).expect("Unable to bind to socket"));
    let listener_thread = {
        let listener = listener.clone();
        let chain = chain.clone();
        thread::spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => {
                        let chain = chain.clone();
                        thread::spawn(|| {
                            handle_peer(stream, chain);
                        });
                        println!("new connection")},
                    Err(e) => writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error"),
                }
            }
        })
    };

    // get peers
    let mut resp = [0; 1024];
    let peer_addrs_str = match nameserver_stream.read(&mut resp) {
        Ok(count) => String::from_utf8_lossy(&resp[0..count]).into_owned(),
        Err(e) => panic!("Error reading from nameserver: {}", e),
    };

    let mut peer_addrs = Vec::new();
    for peer_addr_str in peer_addrs_str.lines() {
        // HACK(zach): yuck, if there are no peers, we send an empty string (otherwise the read blocks)
        if peer_addr_str == "" {
            continue
        }
        let mut resolved = peer_addr_str.to_socket_addrs().unwrap();
        assert!(resolved.len() == 1, "Entry from nameserver resolved to multiple addresses");
        peer_addrs.push(resolved.next().unwrap());
    }

    // inform nameserver
    let my_addr = listener.local_addr().expect("Couldn't get listening address");
    nameserver_stream.write(&NameServerMessage::Inform(my_addr).encode());

    // connect to peers
    let mut peers = Vec::new();
    for addr in peer_addrs {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                peers.push(Connection::new(stream));
            }
            Err(e) => {
                println!("{} is dead: {}", addr, e);
            }
        }
    }

    // get chains from peers
    let mut candidate = {
        let chain = chain.lock().unwrap();
        chain.clone()
    };
    for mut peer in peers {
        peer.write_message(&ClientMessage::QueryChain).unwrap();
        if let ClientMessage::Chain(chain) = peer.read_message().unwrap() {
            if check_chain(&chain) {
                if chain.len() > candidate.len() {
                    candidate = chain;
                } else if chain.len() == candidate.len() &&
                          chain.last().unwrap().timestamp < candidate.last().unwrap().timestamp
                {
                    candidate = chain;
                }
            }
        } else {
            panic!("Unexpected message from peer");
        }
    }
    *chain.lock().unwrap() = candidate;

    // launch repl
    let repl_thread = {
        let chain = chain.clone();
        thread::spawn(move || {
            loop {
                print!("> ");
                std::io::stdout().flush().unwrap();
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).unwrap();
                match parse(input) {
                    Ok(ReplCommand::ShowChain) => {println!("{:#?}", *chain.lock().unwrap());}
                    Ok(ReplCommand::NewBlock) => {
                        let mut chain = chain.lock().unwrap();
                        let new_block = Block::new(chain.last().unwrap(), [0; 1024]);
                        let block_num = new_block.block_num;
                        chain.push(new_block);
                        println!("Created block {}", block_num);
                    },
                    Ok(ReplCommand::Exit) => {std::process::exit(0);}
                    Ok(_) => {println!("Unhandled command");}
                    Err(e) => {println!("Error: {}", e);}
                }
            }
        })
    };




    // let path = Path::new("zach.chain");
    //
    // let mut file = match File::create(&path) {
    //     Err(why) => panic!("couldn't create {}: {}", path.display(), why.description()),
    //     Ok(file) => file,
    // };
    //
    // println!("Hello, world!");
    //
    // let genesis = Block::genesis();
    // println!("genesis: {:#?}", genesis);
    //
    // let block1 = Block::new(&genesis, [0; 1024]);
    // println!("block 1: {:#?}", block1);
    //
    // let block2 = Block::new(&block1, [0; 1024]);
    // println!("block 2: {:#?}", block2);
    //
    // let chain = vec![genesis, block1, block2];
    // println!("chain checks? {:?}", check_chain(&chain));
    //
    // let serialized = serialize(&chain, Infinite).unwrap();
    //
    //
    // match file.write_all(&serialized) {
    //     Err(why) => panic!("couldn't write to {}: {}", path.display(), why.description()),
    //     Ok(_) => println!("successfully wrote"),
    // }
    //
    // let deserialized : Vec<Block> = deserialize(&serialized).unwrap();
    // println!("deserialized successfully? {:#?}", deserialized);

    repl_thread.join();
    listener_thread.join();
}
