use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process;
use std::thread;

fn handle_client(stream: UnixStream) -> Result<(), Box<dyn Error>> {
    let mut reply = stream.try_clone()?;
    let stream = BufReader::new(stream);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/tmp/log.txt")?;
    for line in stream.lines() {
        let line = line?;
        if line.is_empty() {
            reply.write_all(b"action=DUNNO\n\n")?;
            file.write_all(b"--\n\n")?;
            return Ok(());
        }
        file.write_all(format!("{}\n", line).as_bytes())?;
    }
    Ok(())
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
                thread::spawn(|| match handle_client(stream) {
                    Ok(_) => println!("dispatched"),
                    Err(e) => println!("-----> {}", e),
                });
            }
            Err(err) => {
                println!("Error: {}", err);
                break;
            }
        }
    }
}
