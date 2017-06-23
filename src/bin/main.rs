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
use std::sync::mpsc;

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
           their_chain.last().unwrap().timestamp > my_chain.last().unwrap().timestamp {
            return true;
        }
        return false;
    }
    return false;
}

fn ns_to_spec(ns: u64) -> time::Timespec {
    time::Timespec {
        sec: (ns / 1_000_000_000) as i64,
        nsec: (ns % 1_000_000_000) as i32,
    }
}

enum ReplCommand {
    NewBlock,
    ShowChain,
    Exit,
    ListPeers,
    Latest,
    Help,
}

impl ReplCommand {
    fn variants() -> std::slice::Iter<'static, ReplCommand> {
        static VARIANTS: &'static [ReplCommand] = &[ReplCommand::NewBlock,
                                                    ReplCommand::ShowChain,
                                                    ReplCommand::ListPeers,
                                                    ReplCommand::Latest,
                                                    ReplCommand::Exit,
                                                    ReplCommand::Help];
        return VARIANTS.iter();
    }

    fn parse(input: String) -> Result<ReplCommand, String> {
        match input.split_whitespace().next() {
            Some("block") => Ok(ReplCommand::NewBlock),
            Some("chain") => Ok(ReplCommand::ShowChain),
            Some("exit") => Ok(ReplCommand::Exit),
            Some("peers") => Ok(ReplCommand::ListPeers),
            Some("latest") => Ok(ReplCommand::Latest),
            Some("help") => Ok(ReplCommand::Help),
            Some(_) => Err("Unrecognized input".to_string()),
            None => Err("no input".to_string()),
        }
    }

    fn help_string(&self) -> String {
        match self {
            &ReplCommand::NewBlock => "block - create a new block",
            &ReplCommand::ShowChain => "chain - print the chain",
            &ReplCommand::Exit => "exit - close the client",
            &ReplCommand::ListPeers => "peers - list the connected peers",
            &ReplCommand::Latest => "latest - show some info about the latest block",
            &ReplCommand::Help => "help - display this list",
        }
        .to_string()
    }
}


struct State {
    peers: Vec<Connection>,
    chain: Vec<Block>,
    repl_input_rx: mpsc::Receiver<String>,
    new_connections_rx: mpsc::Receiver<Connection>,
}

impl State {
    fn poll_repl(&mut self) -> Result<(), mpsc::TryRecvError>  {
        let input = self.repl_input_rx.try_recv()?;

        match ReplCommand::parse(input) {
            Ok(ReplCommand::ShowChain) => {
                println!("{:#?}", self.chain);
            }
            Ok(ReplCommand::NewBlock) => {
                let new_block = Block::new(self.chain.last().unwrap(), [0; 1024]);
                let block_num = new_block.block_num;
                self.chain.push(new_block.clone());

                {
                    for peer in self.peers.iter_mut() {
                        peer.write_message(&ClientMessage::NewBlock(new_block.clone()));
                    }
                }
                println!("Created block {}", block_num);
            }
            Ok(ReplCommand::Help) => {
                for variant in ReplCommand::variants() {
                    println!("{}", variant.help_string());
                }
            }
            Ok(ReplCommand::ListPeers) => {
                for peer in self.peers.iter() {
                    println!("{}", peer.peer_addr().unwrap());
                }
            }
            Ok(ReplCommand::Latest) => {
                let last = self.chain.last().unwrap();
                println!("Block number {} created at {}",
                         last.block_num,
                         time::at(ns_to_spec(last.timestamp)).rfc822());
            }
            Ok(ReplCommand::Exit) => {
                std::process::exit(0);
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
        print!("> ");
        std::io::stdout().flush().unwrap();
        Ok(())
    }

    fn add_peer(&mut self, mut peer: Connection) {
        println!("!!!!!");
        peer.write_message(&ClientMessage::QueryChain).unwrap();
        println!("Added peer");
        self.peers.push(peer);
    }

    fn poll_listener(&mut self) -> Result<(), mpsc::TryRecvError> {
        let peer = self.new_connections_rx.try_recv()?;
        self.add_peer(peer);
        Ok(())
    }

    fn poll_peers(&mut self) {
        for mut peer in self.peers.iter_mut() {
            let incoming = peer.read_message();
            match incoming {
                Ok(msg) => {
                    match msg {
                        Some(ClientMessage::QueryChain) => {
                            peer.write_message(&ClientMessage::Chain(self.chain.clone()));
                        }
                        Some(ClientMessage::Chain(their_chain)) => {
                            if is_chain_better(&their_chain, &self.chain) {
                                println!("Accepted new chain from {}", peer.peer_addr().unwrap());
                                self.chain = their_chain;
                            }
                        }
                        Some(ClientMessage::NewBlock(block)) => {
                            if block.previous_hash == self.chain.last().unwrap().hash {
                                println!("Received block {} from {}",
                                         block.block_num,
                                         peer.peer_addr().unwrap());
                                self.chain.push(block);
                            } else {
                                // something weird is going on, better check their whole chain
                                println!("Received a block which doesn't match our chain");
                                peer.write_message(&ClientMessage::QueryChain);
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        continue;
                    } else {
                        println!("breaking: {:?}", e.kind());
                        break;
                    }
                }
            }
        }
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
    let nameserver_stream = TcpStream::connect(nameserver_str)
                                    .expect("Couldn't connet to nameserver");
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
    let chain: Vec<Block> = if chain_serialized.len() == 0 {
        vec![Block::genesis()]
    } else {
        deserialize(&chain_serialized).unwrap()
    };

    // listen for peers
    let listener = TcpListener::bind(("::", 0)).expect("Unable to bind to socket");

    let (new_connections_tx, new_connections_rx) = mpsc::channel();

    let my_addr = listener.local_addr().expect("Couldn't get listening address");
    let listener_thread = {
        thread::spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => {
                        stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                        let connection = Connection::new(stream);
                        println!("new connection");
                        new_connections_tx.send(connection);
                    }
                    Err(e) => writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error"),
                }
            }
        })
    };

    let (repl_input_tx, repl_input_rx) = mpsc::channel();
    let repl_thread = {
        thread::spawn(move ||{
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                repl_input_tx.send(line.unwrap());
            }
        })
    };

    let mut state = State {
        peers: vec![],
        new_connections_rx,
        chain,
        repl_input_rx,
    };

    // initialize peers from name server
    nameserver_connection.write_message(&ClientToNameserverMessage::Query);
    let peer_addrs = match nameserver_connection.read_message() {
        Ok(Some(NameserverToClientMessage::Peers(peers))) => peers,
        Ok(_) => panic!("Unexpected message from the nameserver"),
        Err(e) => panic!("Error reading from nameserver: {}", e),
    };

    for addr in peer_addrs {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                let peer = Connection::new(stream);
                state.add_peer(peer);
            }
            Err(e) => {
                println!("{} is dead: {}", addr, e);
            }
        }
    }
    println!("Initialized peers");



    // inform nameserver
    nameserver_connection.write_message(&ClientToNameserverMessage::Inform(my_addr.port()));
    println!("Registered with name server");



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
    //


    println!(">");
    loop {
        state.poll_repl();
        state.poll_listener();
        state.poll_peers();
    }

    listener_thread.join();
    repl_thread.join();
}
