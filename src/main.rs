use clap::{App, Arg};
//use dsn;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process;
use std::thread;

fn main() {
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::with_name("dsn")
                .env("DSN")
                .help("mysql://<username>:<password>@<host>:<port>/<database>")
                .long("dsn")
                .short("d")
                .required(true),
        )
        .arg(
            Arg::with_name("socket")
                .default_value("/tmp/policy-rate-limit.sock")
                .help("path to Unix domain socket")
                .long("socket")
                .short("s"),
        )
        .get_matches();

    let socket_path = matches.value_of("socket").unwrap();
    let dsn = matches.value_of("dsn").unwrap();
    let dsn = dsn::parse(dsn).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    let mut opts = mysql::OptsBuilder::new();
    opts.user(dsn.username);
    opts.pass(dsn.password);
    opts.ip_or_hostname(dsn.host);
    if let Some(port) = dsn.port {
        opts.tcp_port(port);
    }
    opts.socket(dsn.socket);
    opts.db_name(dsn.database);
    //    let opts: mysql::Opts = opts.into();
    let pool = mysql::Pool::new_manual(1, 3, opts).unwrap_or_else(|e| {
        eprintln!("Could not connect to MySQL: {}", e);
        process::exit(1);
    });

    println!("{:?}", pool);
    //    println!("{:?}", opts);

    drop(std::fs::remove_file(socket_path));
    let listener = UnixListener::bind(socket_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| match handle_client(stream) {
                    Err(e) => println!("{}", e),
                    _ => (),
                });
            }
            Err(err) => {
                println!("Error: {}", err);
                break;
            }
        }
    }
}

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
