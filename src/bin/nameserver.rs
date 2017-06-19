use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::io::Read;
use std::io::Write;
use std::env;
use std::sync::Arc;
use std::net::ToSocketAddrs;
use std::sync::RwLock;


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
                thread::spawn(|| {
                    handle_client(stream, wallets);
                });
            }
            Err(e) => writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error"),
        }
    }
}
fn handle_client(mut stream: TcpStream, wallets: Arc<RwLock<Vec<SocketAddr>>>) {
    let mut buffer = [0; 128];

    println!("connection accepted");
    {
        let wallets = wallets.read().unwrap();
        stream.write(wallets.iter().map(|w| format!("{}", w)).collect::<Vec<String>>().join("\n").as_bytes()).unwrap();
        stream.write("\n".as_bytes());
    }
    loop {
        if let Ok(read) = stream.read(&mut buffer) {
            if read == 0 {
                break;
            }
            if let Ok(s) = String::from_utf8(buffer[0..read].to_vec()) {
                let mut tokens = s.split_whitespace();
                match (tokens.next(), tokens.next()) {
                    (Some("i"), Some(address)) => {
                        // probably should get ip from client, not just blindly accept
                        match address.to_string().to_socket_addrs() {
                            Ok(mut addrs) => {
                                let mut wallets = wallets.write().unwrap();
                                let canonical_addr = addrs.next().expect("Resolved to no addresses");
                                wallets.push(canonical_addr);

                                stream.write(wallets.iter().map(|w| format!("{}\n", w)).collect::<Vec<String>>().join("").as_bytes()).unwrap();
                            },
                            Err(e) => {
                                writeln!(std::io::stderr(), "{}", e).expect("Couldn't write error")
                            }
                        }
                    }
                    (Some("q"), _) => {
                        let wallets = wallets.read().unwrap();
                        stream.write(wallets.iter().map(|w| format!("{}\n", w)).collect::<Vec<String>>().join("").as_bytes()).unwrap();
                    }
                    // some way to remove addresses
                    _ => continue
                };
            }
        } else {
            break;
        }
    }
    println!("disconnected")
}
