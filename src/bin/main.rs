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

extern crate naivechain_rs;
use naivechain_rs::block::{check_chain, Block};

use naivechain_rs::message;
use message::{ClientMessage, ClientToNameserverMessage, NameserverToClientMessage};

use naivechain_rs::connection;
use connection::Connection;


fn print_usage(program: &str, opts: getopts::Options) {
    let brief = format!("Usage: {} OPTIONS", program);
    print!("{}", opts.usage(&brief));
}

fn is_chain_better(their_chain: &Vec<Block>, my_chain: &Vec<Block>) -> bool {
    if check_chain(their_chain) {
        if their_chain.len() > my_chain.len() {
            return true;
        }
        if their_chain.len() == my_chain.len() &&
           their_chain.last().unwrap().timestamp > my_chain.last().unwrap().timestamp
        {
            return true;
        }
        return false;
    }
    return false;
}

fn handle_peer(connection: Arc<Mutex<Connection>>, chain: Arc<Mutex<Vec<Block>>>) {
    {
        let connection = connection.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(100));
                // println!("reading!");
                let mut connection = connection.lock().unwrap();
                let incoming = connection.read_message();
                match incoming {
                    Ok(msg) => match msg {
                        Some(ClientMessage::QueryChain) => {
                            let chain = chain.lock().unwrap();
                            connection.write_message(&ClientMessage::Chain(chain.clone()));
                        },
                        Some(ClientMessage::Chain(their_chain)) => {
                            {
                                let mut my_chain = chain.lock().unwrap();
                                if is_chain_better(&their_chain, &my_chain) {
                                    *my_chain = their_chain;
                                }
                            }
                        },
                        Some(ClientMessage::NewBlock(block)) => {
                            let mut chain = chain.lock().unwrap();
                            if block.previous_hash == chain.last().unwrap().hash {
                                println!("Received block {}", block.block_num);
                                chain.push(block);
                            } else {
                                // something weird is going on, better check their whole chain
                                connection.write_message(&ClientMessage::QueryChain);
                            }
                        }
                        None => {
                            break;
                        },
                    },
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            continue;
                        } else {
                            println!("breaking: {:?}", e.kind());
                            break
                        }
                    }
                }
            }
        });
    }
    let mut connection = connection.lock().unwrap();
    connection.write_message(&ClientMessage::QueryChain).unwrap();
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
    let mut nameserver_connection = Connection::new(nameserver_stream);

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

    let mut peers: Arc<Mutex<Vec<Arc<Mutex<Connection>>>>> = Arc::new(Mutex::new(Vec::new()));

    // listen for peers
    let listener = Arc::new(TcpListener::bind(("::", 0)).expect("Unable to bind to socket"));

    // get peers
    nameserver_connection.write_message(&ClientToNameserverMessage::Query);
    let peer_addrs = match nameserver_connection.read_message() {
        Ok(Some(NameserverToClientMessage::Peers(peers))) => peers,
        Ok(_) => panic!("Unexpected message from the nameserver"),
        Err(e) => panic!("Error reading from nameserver: {}", e),
    };

    {
        let mut peers = peers.lock().unwrap();
        for addr in peer_addrs {
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                    let connection = Arc::new(Mutex::new(Connection::new(stream)));
                    {
                        let chain = chain.clone();
                        let connection = connection.clone();
                        handle_peer(connection, chain);
                    }
                    peers.push(connection);
                }
                Err(e) => {
                    println!("{} is dead: {}", addr, e);
                }
            }
        }
    }

    let listener_thread = {
        let listener = listener.clone();
        let chain = chain.clone();
        let peers = peers.clone();
        thread::spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => {
                        stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                        let connection = Arc::new(Mutex::new(Connection::new(stream)));
                        {
                            let mut peers = peers.lock().unwrap();
                            let connection = connection.clone();
                            peers.push(connection);
                        }
                        {
                            let chain = chain.clone();
                            handle_peer(connection, chain);
                        }
                        println!("new connection")},
                    Err(e) => writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error"),
                }
            }
        })
    };

    // inform nameserver
    let my_addr = listener.local_addr().expect("Couldn't get listening address");
    nameserver_connection.write_message(&ClientToNameserverMessage::Inform(my_addr.port()));

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
                        chain.push(new_block.clone());

                        {
                            let peers = peers.lock().unwrap();
                            for peer in peers.iter() {
                                let mut peer = peer.lock().unwrap();
                                peer.write_message(&ClientMessage::NewBlock(new_block.clone()));
                            }
                        }
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
