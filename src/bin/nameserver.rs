use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::io::Read;
use std::io::Write;
use std::env;
use std::sync::Arc;
use std::net::ToSocketAddrs;
use std::sync::RwLock;
use std::collections::HashMap;

extern crate naivechain_rs;
use naivechain_rs::message::{ClientMessage, ClientToNameserverMessage, NameserverToClientMessage};
use naivechain_rs::connection::Connection;


fn main() {
    let args: Vec<String> = env::args().collect();
    let port: u16 = args.get(1).map(|port_string|
        match port_string.parse::<u16>() {
            Ok(i) => i,
            Err(_) => panic!("Port number must be an integer < 65536")
        }
    ).unwrap_or(18000);

    let listener = TcpListener::bind(("::", port)).expect("Unable to bind to socket");
    let addr = listener.local_addr().expect("unable to get the local port?");

    let wallets: Arc<RwLock<HashMap<SocketAddr, SocketAddr>>> = Arc::new(RwLock::new(HashMap::new()));

    println!("Listening on port {}", addr.port());
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let wallets = wallets.clone();
                let connection = Connection::new(stream);
                thread::spawn(|| {
                    handle_client(connection, wallets);
                });
            }
            Err(e) => writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error"),
        }
    }
}
fn handle_client(mut connection: Connection, wallets: Arc<RwLock<HashMap<SocketAddr, SocketAddr>>>) {
    let mut remote_addr;
    match connection.peer_addr() {
        Ok(addr) => {
            remote_addr = addr;
            let mut wallets = wallets.write().unwrap();
            println!("Met {}", addr);
        },
        Err(e) => {
            writeln!(std::io::stderr(), "Error getting peer address: {}", e).expect("Couldn't write error");
            return;
        },
    }
    loop {
        match connection.read_message() {
            Ok(Some(ClientToNameserverMessage::Inform(socket_addr))) => {
                let mut wallets = wallets.write().unwrap();
                wallets.insert(remote_addr, socket_addr);
            },
            Ok(Some(ClientToNameserverMessage::Query)) => {
                let wallets = wallets.read().unwrap();
                let mut peer_addresses = wallets.clone();
                peer_addresses.remove(&remote_addr);

                connection.write_message(
                    &NameserverToClientMessage::Peers(
                        peer_addresses.into_iter().map(|(_, v)| v).collect()
                    )
                ).unwrap();
            },
            Ok(None) => break,
            Ok(_) => panic!("Unexpected client message"),
            Err(e) => panic!("Error reading client message: {}", e),
        }
    }
    {
        let mut wallets = wallets.write().unwrap();
        wallets.remove(&remote_addr);
        println!("Lost {}", remote_addr);
    }
}
