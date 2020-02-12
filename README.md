# policyd-rate-limit

[![crates.io](https://img.shields.io/crates/v/policyd-rate-limit.svg)](https://crates.io/crates/policyd-rate-limit)
[![Build Status](https://travis-ci.org/nbari/policyd-rate-limit.svg?branch=master)](https://travis-ci.org/nbari/policyd-rate-limit)

Postfix rate limiter SMTP policy daemon

# How it works

It depends on the [Postfix policy delegation protocol](http://www.postfix.org/SMTPD_POLICY_README.html), it searches for the `sasl_username` and based on the defined limits stored in a MySQl database it rejects or allows `action=DUNNO` the email to be sent.

# How to use

```txt
USAGE:
    policyd-rate-limit [OPTIONS] --dsn <dsn> [SUBCOMMAND]

FLAGS:
        --debug      Prints all input from client
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --dsn <dsn>          mysql://<username>:<password>@tcp(<host>:<port>)/<database>
        --max <max>          mysql pool max connections [default: 50]
        --min <min>          mysql pool min connections [default: 3]
    -s, --socket <socket>    path to Unix domain socket [default: /tmp/policy-rate-limit.sock]

SUBCOMMANDS:
    cuser    Create the user if not found, defaults: 100 messages per day
    help     Prints this message or the help of the given subcommand(s)
```

For the subcommand `cuser`:

```txt
Create the user if not found, defaults: 100 messages per day

USAGE:
    policyd-rate-limit --dsn <dsn> cuser [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -l, --limit <limit>    maximum allowed messages [default: 100]
    -r, --rate <rate>      rate in seconds, limits the messages to be sent in the defined period [default: 86400]
```

Use a supervisor ([immortal](https://immortal.run)) to run `policyd-rate-limit`,
for example to create users if not found and to only allow 3 emails every hour
use:

    policyd-rate-limit -d mysql://root:test@tcp(localhost)/policyd -s /var/run/policy-rate-limit.sock cuser -l 3 -r 3600

The database schema:


```sql
CREATE SCHEMA IF NOT EXISTS `policyd` DEFAULT CHARACTER SET utf8 COLLATE utf8_general_ci;

USE policyd;

CREATE TABLE IF NOT EXISTS `ratelimit` (
	`username` VARCHAR(128) NOT NULL COMMENT 'sender address (SASL username)',
	`quota` INT(10) UNSIGNED NOT NULL DEFAULT '0' COMMENT 'limit',
	`used` INT(10) UNSIGNED NOT NULL DEFAULT '0' COMMENT 'current recipient counter',
	`rate` INT(10) UNSIGNED DEFAULT '0' COMMENT 'seconds after which the counter gets reset',
	`rdate` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT 'datetime when counter was reset',
	PRIMARY KEY (`username`))
ENGINE = InnoDB
DEFAULT CHARACTER SET = utf8
COLLATE = utf8_general_ci;
```

# Postfix configuration

Add the path of the policy-rate-limit socket to `smtpd_sender_restrictions` for example:

    smtpd_sender_restrictions: check_policy_service { unix:/tmp/policy-rate-limit.sock, default_action=DUNNO }

> check the perms of the socket, you may need `chmod 666`
