# policyd-rate-limit

[![crates.io](https://img.shields.io/crates/v/policyd-rate-limit.svg)](https://crates.io/crates/policyd-rate-limit)
[![Test](https://github.com/nbari/policyd-rate-limit/actions/workflows/test.yml/badge.svg)](https://github.com/nbari/policyd-rate-limit/actions/workflows/test.yml)

Postfix rate limiter SMTP policy daemon

# How it works

It depends on the [Postfix policy delegation protocol](http://www.postfix.org/SMTPD_POLICY_README.html), it searches for the `sasl_username` and based on the defined limits stored in a SQL(MySQL/PostgreSQL/SQLite) database it rejects or allows `action=DUNNO` the email to be sent.

# How to use

```txt
Postfix policy daemon for rate limiting

Usage: policyd-rate-limit [OPTIONS] --dsn <dsn>

Options:
  -s, --socket <SOCKET>  Path to the Unix domain socket [default: /tmp/policy-rate-limit.sock]
      --dsn <dsn>        Database connection string [env: DSN=]
      --pool <pool>      Pool size for database connections [default: 5]
  -l, --limit <limit>    Maximum allowed messages [default: 10]
  -r, --rate <rate>      rate in seconds, limits the messages to be sent in the defined period [default: 86400]
  -v, --verbose...       Increase verbosity, -vv for debug
  -h, --help             Print help
  -V, --version          Print version
```

The database schema (postgres example):

```sql
CREATE TABLE IF NOT EXISTS ratelimit (
    username VARCHAR(128) NOT NULL, -- sender address (SASL username)
    quota INTEGER NOT NULL DEFAULT 0, -- limit
    used INTEGER NOT NULL DEFAULT 0, -- current recipient counter
    rate INTEGER DEFAULT 0, -- seconds after which the counter gets reset
    rdate TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP, -- datetime when counter was reset
    PRIMARY KEY (username)
);
```

# Postfix configuration

Add the path of the policy-rate-limit socket to `smtpd_sender_restrictions` for example:

    smtpd_sender_restrictions: check_policy_service { unix:/tmp/policy-rate-limit.sock, default_action=DUNNO }

> check the perms of the socket
