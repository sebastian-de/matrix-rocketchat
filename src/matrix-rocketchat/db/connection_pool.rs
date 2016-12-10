use diesel::sqlite::SqliteConnection;
use iron::{Plugin, Request};
use iron::typemap::Key;
use persistent::Write;
use r2d2::{Config, Pool, PooledConnection};
use r2d2_diesel::ConnectionManager;

use errors::*;

/// Struct to attach a database connection pool to an iron request.
pub struct ConnectionPool;

impl ConnectionPool {
    /// Create connection pool for the sqlite database
    pub fn new(database_url: &str) -> Pool<ConnectionManager<SqliteConnection>> {
        let config = Config::default();
        let manager = ConnectionManager::<SqliteConnection>::new(database_url);
        Pool::new(config, manager).expect("Failed to create pool.")
    }

    /// Extract a database connection from the pool stored in the request.
    pub fn get_from_request(request: &mut Request) -> Result<PooledConnection<ConnectionManager<SqliteConnection>>> {
        let mutex = request.get::<Write<ConnectionPool>>().unwrap();
        let pool = mutex.lock().unwrap();
        pool.get().chain_err(|| "Could not get connection from connection pool")
    }
}

impl Key for ConnectionPool {
    type Value = Pool<ConnectionManager<SqliteConnection>>;
}
