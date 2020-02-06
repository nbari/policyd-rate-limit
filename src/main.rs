use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;

fn handle_client(stream: UnixStream) {
    let stream = BufReader::new(stream);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/tmp/log.txt")
        .expect("cannot open file");
    for line in stream.lines() {
        file.write_all(line.unwrap().as_bytes())
            .expect("write failed");
    }
}

fn main() {
    let listener = UnixListener::bind("/tmp/policy-rate-limit.sock").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| handle_client(stream));
            }
            Err(err) => {
                println!("Error: {}", err);
                break;
            }
        }
    }
}
