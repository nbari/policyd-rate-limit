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
