Changelog
=========

## 1.0.1
- improve reset_quota_if_expired to using CTE to get NOW() and reduce function calls

## 1.0.0
- using sqlx to support multiple database backends
- using tokio to support async
- simplify the arguments to the command line by using limit and rate directly
