use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::io::Read;
use std::io::Write;
use std::env;
use std::sync::Arc;
use std::net::ToSocketAddrs;
use std::sync::RwLock;

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

    let wallets: Arc<RwLock<Vec<SocketAddr>>> = Arc::new(RwLock::new(Vec::new()));

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
fn handle_client(mut connection: Connection, wallets: Arc<RwLock<Vec<SocketAddr>>>) {
    println!("connection accepted");
    loop {
        match connection.read_message() {
            Ok(Some(ClientToNameserverMessage::Inform(socket_addr))) => {
                let mut wallets = wallets.write().unwrap();
                wallets.push(socket_addr);
            },
            Ok(Some(ClientToNameserverMessage::Query)) => {
                let wallets = wallets.read().unwrap();
                connection.write_message(&NameserverToClientMessage::Peers(wallets.clone())).unwrap();
            },
            Ok(None) => break,
            Ok(_) => panic!("Unexpected client message"),
            Err(e) => panic!("Error reading client message: {}", e),
        }
    }
    println!("disconnected")
}
