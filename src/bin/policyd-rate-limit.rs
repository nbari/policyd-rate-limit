use clap::{App, Arg, SubCommand};
use policyd_rate_limit::queries;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process;
use std::thread;

#[derive(Debug, Default, Clone)]
pub struct CreateUser {
    limit: Option<usize>,
    rate: Option<usize>,
}

fn is_num(s: String) -> Result<(), String> {
    if let Err(..) = s.parse::<usize>() {
        return Err(String::from("Not a valid number!"));
    }
    Ok(())
}

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
            Arg::with_name("min")
                .default_value("3")
                .help("mysql pool min connections")
                .long("min")
                .validator(is_num),
        )
        .arg(
            Arg::with_name("max")
                .default_value("50")
                .help("mysql pool max connections")
                .long("max")
                .validator(is_num),
        )
        .arg(
            Arg::with_name("socket")
                .default_value("/tmp/policy-rate-limit.sock")
                .help("path to Unix domain socket")
                .long("socket")
                .short("s"),
        )
        .subcommand(
            SubCommand::with_name("cuser")
                .about("Create the user if not found, defaults: 100 messages per day")
                .arg(
                    Arg::with_name("limit")
                        .default_value("100")
                        .help("maximum allowed messages")
                        .long("limit")
                        .short("l")
                        .validator(is_num),
                )
                .arg(
                    Arg::with_name("rate")
                        .default_value("86400")
                        .help(
                            "rate in seconds, limits the messages to be sent in the defined period",
                        )
                        .long("rate")
                        .short("r")
                        .validator(is_num),
                ),
        )
        .get_matches();

    // if cuser, create the user if not found in the DB
    let cuser = if let Some(m) = matches.subcommand_matches("cuser") {
        CreateUser {
            limit: Some(m.value_of("limit").unwrap().parse::<usize>().unwrap()),
            rate: Some(m.value_of("rate").unwrap().parse::<usize>().unwrap()),
        }
    } else {
        CreateUser::default()
    };

    let socket_path = matches.value_of("socket").unwrap();

    // prepare DSN for the mysql pool
    let dsn = matches.value_of("dsn").unwrap();
    let dsn = dsn::parse(dsn).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let pool_min = matches.value_of("min").unwrap().parse::<usize>().unwrap();
    let pool_max = matches.value_of("max").unwrap().parse::<usize>().unwrap();

    let mut opts = mysql::OptsBuilder::new();
    opts.user(dsn.username);
    opts.pass(dsn.password);
    opts.ip_or_hostname(dsn.host);
    if let Some(port) = dsn.port {
        opts.tcp_port(port);
    }
    opts.socket(dsn.socket);
    opts.db_name(dsn.database);
    let pool = mysql::Pool::new_manual(pool_min, pool_max, opts).unwrap_or_else(|e| {
        eprintln!("Could not connect to MySQL: {}", e);
        process::exit(1);
    });

    // remove existing socket file if exists
    drop(std::fs::remove_file(socket_path));
    let listener = UnixListener::bind(socket_path).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    // start to listen in the socket
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let pool = pool.clone();
                let cuser = cuser.clone();
                thread::spawn(|| {
                    let mut reply = stream.try_clone().unwrap();
                    if let Err(e) = handle_client(stream, queries::new(pool), cuser) {
                        drop(reply.write_all(b"action=DUNNO\n\n"));
                        println!("{}", e)
                    }
                });
            }
            Err(err) => {
                println!("Error: {}", err);
                break;
            }
        }
    }
}

fn handle_client(
    stream: UnixStream,
    pool: queries::Queries,
    cuser: CreateUser,
) -> Result<(), Box<dyn Error>> {
    println!("{:?} {:?}", pool.pool, cuser);
    let mut reply = stream.try_clone()?;
    let stream = BufReader::new(stream);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/tmp/log.txt")?;

    // search for sasl_username
    for line in stream.lines() {
        let line = line?;
        if line.starts_with("sasl_username=") {
            let sasl_username = line.rsplit('=').take(1).collect::<Vec<_>>()[0];
            if sasl_username.is_empty() {
                reply.write_all(b"action=DUNNO\n\n")?;
                return Ok(());
            }
            println!("{:#?}", cuser);
            // find username
            match pool.get_user(sasl_username) {
                Ok(n) => println!("{}", n),
                Err(_) => {
                    if let Some(limit) = cuser.limit {
                        if let Some(rate) = cuser.rate {
                            pool.create_user(sasl_username, limit, rate)?;
                        }
                    }
                }
            }
        } else if line.is_empty() {
            reply.write_all(b"action=DUNNO\n\n")?;
            file.write_all(b"--\n\n")?;
            return Ok(());
        }
        file.write_all(format!("{}\n", line).as_bytes())?;
    }
    Ok(())
}