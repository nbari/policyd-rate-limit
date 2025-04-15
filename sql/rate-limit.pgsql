CREATE TABLE IF NOT EXISTS ratelimit (
    username VARCHAR(128) NOT NULL, -- sender address (SASL username)
    quota INTEGER NOT NULL DEFAULT 0, -- limit
    used INTEGER NOT NULL DEFAULT 0, -- current recipient counter
    rate INTEGER DEFAULT 0, -- seconds after which the counter gets reset
    rdate TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP, -- datetime when counter was reset
    PRIMARY KEY (username)
);
