use std::io::Write;
use std::net::TcpListener;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8082").unwrap();
    println!("http://localhost:8082");
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let _ = write!(s, "HTTP/1.1 200 OK\r\n\r\nrust-app running");
            let _ = s.flush();
        }
    }
}
