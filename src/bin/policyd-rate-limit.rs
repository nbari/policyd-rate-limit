use clap::{App, Arg, SubCommand};
use policyd_rate_limit::queries;
use std::error::Error;
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
                .help("mysql://<username>:<password>@tcp(<host>:<port>)/<database>")
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
        .arg(
            Arg::with_name("debug")
                .help("Prints all input from client")
                .long("debug"),
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

    let debug = matches.is_present("debug");

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
    opts.pass(dsn.password.clone());
    opts.ip_or_hostname(dsn.host);
    if let Some(port) = dsn.port {
        opts.tcp_port(port);
    }
    opts.socket(dsn.socket);
    opts.db_name(dsn.database);

    // mysql ssl options
    let mut ssl_opts = mysql::SslOpts::default();
    if let Some(tls) = dsn.params.get("tls") {
        if *tls == "skip-verify" {
            ssl_opts.set_danger_accept_invalid_certs(true);
        }
    }
    opts.ssl_opts(ssl_opts);

    let pool = mysql::Pool::new_manual(pool_min, pool_max, opts).unwrap_or_else(|e| {
        eprintln!("Could not connect to MySQL: {}", e);
        process::exit(1);
    });

    if debug {
        println!("{:?}", pool);
    }

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
                thread::spawn(move || {
                    let mut reply = stream.try_clone().unwrap();
                    if let Err(e) = handle_client(stream, &queries::new(pool), &cuser, debug) {
                        drop(reply.write_all(b"action=DUNNO\n\n"));
                        println!("Error: {}", e)
                    }
                });
            }
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        }
    }
}

fn handle_client(
    stream: UnixStream,
    pool: &queries::Queries,
    cuser: &CreateUser,
    debug: bool,
) -> Result<(), Box<dyn Error>> {
    let mut reply = stream.try_clone()?;
    let stream = BufReader::new(stream);

    // search for sasl_username
    for line in stream.lines() {
        let line = line?;
        if debug {
            println!("{}", line);
        }
        if line.starts_with("sasl_username=") {
            let sasl_username = line.rsplit('=').take(1).collect::<Vec<_>>()[0];
            if sasl_username.is_empty() {
                reply.write_all(b"action=DUNNO\n\n")?;
                return Ok(());
            }
            // find username
            if let Ok(ok) = pool.get_user(sasl_username) {
                if ok {
                    // allow sending since the user has not reached the limits/quota
                    reply.write_all(b"action=DUNNO\n\n")?;
                } else {
                    // check if the rate has expired and if yes reset limits and allow sending
                    if pool.reset_quota(sasl_username)? > 0 {
                        reply.write_all(b"action=DUNNO\n\n")?;
                    } else {
                        reply.write_all(b"action=REJECT\n\n")?;
                    }
                }
                pool.update_quota(sasl_username)?;
                return Ok(());
            } else {
                // create user if cuser subcommand defined
                if let Some(limit) = cuser.limit {
                    if let Some(rate) = cuser.rate {
                        pool.create_user(sasl_username, limit, rate)?;
                    }
                }
                reply.write_all(b"action=DUNNO\n\n")?;
                return Ok(());
            }
        } else if line.is_empty() {
            reply.write_all(b"action=DUNNO\n\n")?;
            return Ok(());
        }
    }
    Ok(())
}
