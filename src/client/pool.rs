//! Client Connection Pooling
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, Shutdown};
use std::sync::{Arc, Mutex};

use mio::{Evented, Selector, Token, EventSet, PollOpt};
use tick::Slab;


use http;
use net;

pub struct Pool {
    connections: Slab<()>,
}

/// Config options for the `Pool`.
#[derive(Debug)]
pub struct Config {
    /// The maximum idle connections *per host*.
    pub max_idle: usize,
    pub max_connections: usize
}

impl Default for Config {
    #[inline]
    fn default() -> Config {
        Config {
            max_idle: 5,
            max_connections: 8_192
        }
    }
}

impl Pool {
    /// Creates a `Pool` with a specified `NetworkConnector`.
    #[inline]
    pub fn new(config: Config) -> Pool {
        Pool {
            connections: Slab::new(config.max_connections),
        }
    }
}

impl http::Handler for Pool {
    type Incoming = ::httparse::Response<'static, 'static>;
    type Outgoing = http::Request;

    fn on_incoming(&mut self, incoming: http::IncomingResponse, stream: http::Stream, transfer: http::Transfer<http::Request, net::Fresh>) {
    
    }
}

type Key = (String, u16, Scheme);

fn key<T: Into<Scheme>>(host: &str, port: u16, scheme: T) -> Key {
    (host.to_owned(), port, scheme.into())
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
enum Scheme {
    Http,
    Https,
    Other(String)
}

impl<'a> From<&'a str> for Scheme {
    fn from(s: &'a str) -> Scheme {
        match s {
            "http" => Scheme::Http,
            "https" => Scheme::Https,
            s => Scheme::Other(String::from(s))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Shutdown;
    use std::io::Read;
    use mock::{MockConnector};
    use net::{NetworkConnector, NetworkStream};

    use super::{Pool, key};

    macro_rules! mocked {
        () => ({
            Pool::with_connector(Default::default(), MockConnector)
        })
    }

    #[test]
    fn test_connect_and_drop() {
        let pool = mocked!();
        let key = key("127.0.0.1", 3000, "http");
        pool.connect("127.0.0.1", 3000, "http").unwrap();
        {
            let locked = pool.inner.lock().unwrap();
            assert_eq!(locked.conns.len(), 1);
            assert_eq!(locked.conns.get(&key).unwrap().len(), 1);
        }
        pool.connect("127.0.0.1", 3000, "http").unwrap(); //reused
        {
            let locked = pool.inner.lock().unwrap();
            assert_eq!(locked.conns.len(), 1);
            assert_eq!(locked.conns.get(&key).unwrap().len(), 1);
        }
    }

    #[test]
    fn test_closed() {
        let pool = mocked!();
        let mut stream = pool.connect("127.0.0.1", 3000, "http").unwrap();
        stream.close(Shutdown::Both).unwrap();
        drop(stream);
        let locked = pool.inner.lock().unwrap();
        assert_eq!(locked.conns.len(), 0);
    }

    #[test]
    fn test_eof_closes() {
        let pool = mocked!();

        let mut stream = pool.connect("127.0.0.1", 3000, "http").unwrap();
        assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        drop(stream);
        let locked = pool.inner.lock().unwrap();
        assert_eq!(locked.conns.len(), 0);
    }
}
