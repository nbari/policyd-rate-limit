use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process;
use std::thread;

fn handle_client(stream: UnixStream) {
    let stream = BufReader::new(stream);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/tmp/log.txt")
        .expect("cannot open file");
    for line in stream.lines() {
        file.write_all(format!("{}\n", line.unwrap()).as_bytes())
            .expect("write failed");
    }
    file.write_all(b"---\n\n").unwrap();
}

fn main() {
    drop(std::fs::remove_file("/tmp/policy-rate-limit.sock"));
    let listener = UnixListener::bind("/tmp/policy-rate-limit.sock").unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

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
