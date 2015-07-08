//! HTTP Server
//!
//! # Server
//!
//! A `Server` is created to listen on port, parse HTTP requests, and hand
//! them off to a `Handler`. By default, the Server will listen across multiple
//! threads, but that can be configured to a single thread if preferred.
//!
//! # Handling requests
//!
//! You must pass a `Handler` to the Server that will handle requests. There is
//! a default implementation for `fn`s and closures, allowing you pass one of
//! those easily.
//!
//!
//! ```no_run
//! use hyper::server::{Server, Request, Response};
//!
//! fn hello(req: Request, res: Response) {
//!     // handle things here
//! }
//!
//! Server::http("0.0.0.0:0").unwrap().handle(hello).unwrap();
//! ```
//!
//! As with any trait, you can also define a struct and implement `Handler`
//! directly on your own type, and pass that to the `Server` instead.
//!
//! ```no_run
//! use std::sync::Mutex;
//! use std::sync::mpsc::{channel, Sender};
//! use hyper::server::{Handler, Server, Request, Response};
//!
//! struct SenderHandler {
//!     sender: Mutex<Sender<&'static str>>
//! }
//!
//! impl Handler for SenderHandler {
//!     fn handle(&self, req: Request, res: Response) {
//!         self.sender.lock().unwrap().send("start").unwrap();
//!     }
//! }
//!
//!
//! let (tx, rx) = channel();
//! Server::http("0.0.0.0:0").unwrap().handle(SenderHandler {
//!     sender: Mutex::new(tx)
//! }).unwrap();
//! ```
//!
//! Since the `Server` will be listening on multiple threads, the `Handler`
//! must implement `Sync`: any mutable state must be synchronized.
//!
//! ```no_run
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use hyper::server::{Server, Request, Response};
//!
//! let counter = AtomicUsize::new(0);
//! Server::http("0.0.0.0:0").unwrap().handle(move |req: Request, res: Response| {
//!     counter.fetch_add(1, Ordering::Relaxed);
//! }).unwrap();
//! ```
//!
//! # The `Request` and `Response` pair
//!
//! A `Handler` receives a pair of arguments, a `Request` and a `Response`. The
//! `Request` includes access to the `method`, `uri`, and `headers` of the
//! incoming HTTP request. It also implements `std::io::Read`, in order to
//! read any body, such as with `POST` or `PUT` messages.
//!
//! Likewise, the `Response` includes ways to set the `status` and `headers`,
//! and implements `std::io::Write` to allow writing the response body.
//!
//! ```no_run
//! use std::io;
//! use hyper::server::{Server, Request, Response};
//! use hyper::status::StatusCode;
//!
//! Server::http("0.0.0.0:0").unwrap().handle(|mut req: Request, mut res: Response| {
//!     match req.method {
//!         hyper::Post => {
//!             io::copy(&mut req, &mut res.start().unwrap()).unwrap();
//!         },
//!         _ => *res.status_mut() = StatusCode::MethodNotAllowed
//!     }
//! }).unwrap();
//! ```
//!
//! ## An aside: Write Status
//!
//! The `Response` uses a phantom type parameter to determine its write status.
//! What does that mean? In short, it ensures you never write a body before
//! adding all headers, and never add a header after writing some of the body.
//!
//! This is often done in most implementations by include a boolean property
//! on the response, such as `headers_written`, checking that each time the
//! body has something to write, so as to make sure the headers are sent once,
//! and only once. But this has 2 downsides:
//!
//! 1. You are typically never notified that your late header is doing nothing.
//! 2. There's a runtime cost to checking on every write.
//!
//! Instead, hyper handles this statically, or at compile-time. A
//! `Response<Fresh>` includes a `headers_mut()` method, allowing you add more
//! headers. It also does not implement `Write`, so you can't accidentally
//! write early. Once the "head" of the response is correct, you can "send" it
//! out by calling `start` on the `Response<Fresh>`. This will return a new
//! `Response<Streaming>` object, that no longer has `headers_mut()`, but does
//! implement `Write`.
use std::fmt;
use std::net::{SocketAddr/*, ToSocketAddrs*/};
use std::thread;

use std::time::Duration;

//use num_cpus;

use mio::{TryAccept, Evented};
use tick::{self, Tick};

pub use self::request::Request;
pub use self::response::Response;

use http::{self, Next};
//use net::{HttpsListener, Ssl, HttpsStream};
use net::{HttpListener, HttpStream, Transport};


mod request;
mod response;
mod message;

/// A server can listen on a TCP socket.
///
/// Once listening, it will create a `Request`/`Response` pair for each
/// incoming connection, and hand them to the provided handler.
#[derive(Debug)]
pub struct Server<T: TryAccept + Evented> {
    listener: T,
    timeouts: Timeouts,
}

#[derive(Clone, Copy, Debug, Default)]
struct Timeouts {
    read: Option<Duration>,
    write: Option<Duration>,
    keep_alive: Option<Duration>,
}

macro_rules! try_option(
    ($e:expr) => {{
        match $e {
            Some(v) => v,
            None => return None
        }
    }}
);

impl<T> Server<T> where T: TryAccept + Evented, <T as TryAccept>::Output: Transport {
    /// Creates a new server with the provided handler.
    #[inline]
    pub fn new(listener: T) -> Server<T> {
        Server {
            listener: listener,
            timeouts: Timeouts::default()
        }
    }

    /// Controls keep-alive for this server.
    ///
    /// The timeout duration passed will be used to determine how long
    /// to keep the connection alive before dropping it.
    ///
    /// Passing `None` will disable keep-alive.
    ///
    /// Default is enabled with a 5 second timeout.
    #[inline]
    pub fn keep_alive(&mut self, timeout: Option<Duration>) {
        self.timeouts.keep_alive = timeout;
    }

    /// Sets the read timeout for all Request reads.
    pub fn set_read_timeout(&mut self, dur: Option<Duration>) {
        self.timeouts.read = dur;
    }

    /// Sets the write timeout for all Response writes.
    pub fn set_write_timeout(&mut self, dur: Option<Duration>) {
        self.timeouts.write = dur;
    }
}

impl Server<HttpListener> {
    pub fn http(addr: &str) -> ::Result<Server<HttpListener>> {
        HttpListener::bind(&addr.parse().unwrap())
            .map(Server::new)
            .map_err(From::from)
    }
}


/*
impl<S: Ssl> Server<HttpsStream<S::Stream>> {
    /// Creates a new server that will handle `HttpStream`s over SSL.
    ///
    /// You can use any SSL implementation, as long as implements `hyper::net::Ssl`.
    pub fn https(addr: &SocketAddr, ssl: S) -> ::Result<Server<HttpsListener<S>>> {
        HttpsListener::new(addr, ssl).map(Server::new)
    }
}
*/


//impl<T: Transport> Server<T> {
impl Server<HttpListener> {
    /// Binds to a socket and starts handling connections.
    pub fn handle<H>(self, factory: H) -> ::Result<Listening>
    where H: HandlerFactory<HttpStream> {
    /*
        self.handle_threads(handler, num_cpus::get() * 5 / 4)
    }

    /// Binds to a socket and starts handling connections with the provided
    /// number of threads.
    pub fn handle_threads<H>(self, factory: H, threads: usize) -> ::Result<Listening>
    where H: HandlerFactory<HttpStream> {
        trace!("handle_threads {}", threads);
    */
        let addr = try!(self.listener.local_addr());
        //let factory = ::std::sync::Arc::new(factory);
        //let mut handles = vec![];
        //let mut ticks = vec![];
        let (tx, rx) = ::std::sync::mpsc::channel();
        let listener = self.listener; //try!(self.listener.try_clone());
        let handle = thread::Builder::new().name("hyper-server".to_owned()).spawn(move || {
            let factory = ::std::rc::Rc::new(::std::cell::RefCell::new(factory));
            //for _ in 0..threads {
            //let factory = factory.clone();
            let mut tick = Tick::<HttpListener, _>::new(move |t| {
                trace!("connection accepted");
                let factory = factory.clone();
                let conn = http::Conn::new(t, move || {
                    message::Message::new(factory.borrow_mut().create())
                });
                (conn, tick::Interest::Read)
            });
            tx.send(tick.notify()).unwrap();
            tick.accept(listener).unwrap();
            tick.run().unwrap();
        }).unwrap();

        let tick = rx.recv().unwrap();

        Ok(Listening {
            addr: addr,
            handle: Some((handle, tick)),
        })
    }
}

/// A handle of the running server.
pub struct Listening {
    /// The address this server is listening on.
    pub addr: SocketAddr,
    handle: Option<(::std::thread::JoinHandle<()>, ::tick::Notify)>,
}

impl fmt::Debug for Listening {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Listening")
            .field("addr", &self.addr)
            .finish()
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.handle.take().map(|(handle, _)| {
            handle.join().unwrap();
        });
    }
}

impl Listening {
    /// Starts the Server, blocking until it is shutdown.
    pub fn forever(self) {

    }
    /// Stop the server from listening to its socket address.
    pub fn close(mut self) {
        debug!("closing server");
        self.handle.take().map(|(handle, tick)| {
            tick.shutdown();
            handle.join().unwrap();
        });
    }
}


/*
/// A handler that can handle incoming requests for a server.
pub trait Handler: Sync + Send {
    /// Receives a `Request`/`Response` pair, and should perform some action on them.
    ///
    /// This could reading from the request, and writing to the response.
    fn handle(&self, Request, Response<Fresh>);

    /// Called when a Request includes a `Expect: 100-continue` header.
    ///
    /// By default, this will always immediately response with a `StatusCode::Continue`,
    /// but can be overridden with custom behavior.
    fn check_continue(&self, _: (&Method, &RequestUri, &Headers)) -> StatusCode {
        StatusCode::Continue
    }

    /// This is run after a connection is received, on a per-connection basis (not a
    /// per-request basis, as a connection with keep-alive may handle multiple
    /// requests)
    fn on_connection_start(&self) { }

    /// This is run before a connection is closed, on a per-connection basis (not a
    /// per-request basis, as a connection with keep-alive may handle multiple
    /// requests)
    fn on_connection_end(&self) { }
}

impl<F> Handler for F where F: Fn(Request, Response<Fresh>), F: Sync + Send {
    fn handle(&self, req: Request, res: Response<Fresh>) {
        self(req, res)
    }
}
*/

pub trait Handler<T: Transport>: Send + 'static {
    fn on_request(&mut self, request: Request) -> Next;
    fn on_request_readable(&mut self, request: &mut http::Decoder<T>) -> Next;
    fn on_response(&mut self, response: &mut Response) -> Next;
    fn on_response_writable(&mut self, response: &mut http::Encoder<T>) -> Next;
}


pub trait HandlerFactory<T: Transport>: Send + 'static {
    type Output: Handler<T>;
    fn create(&mut self) -> Self::Output;
}

impl<F, H, T> HandlerFactory<T> for F
where F: FnMut() -> H + Send + 'static, H: Handler<T>, T: Transport {
    type Output = H;
    fn create(&mut self) -> H {
        self()
    }
}

#[cfg(test)]
mod tests {
    /*
    use header::Headers;
    use method::Method;
    use mock::MockStream;
    use status::StatusCode;
    use uri::RequestUri;

    use super::{Request, Response, Fresh, Handler, Worker};

    #[test]
    fn test_check_continue_default() {
        let mut mock = MockStream::with_input(b"\
            POST /upload HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Expect: 100-continue\r\n\
            Content-Length: 10\r\n\
            \r\n\
            1234567890\
        ");

        fn handle(_: Request, res: Response<Fresh>) {
            res.start().unwrap().end().unwrap();
        }

        Worker::new(handle, Default::default()).handle_connection(&mut mock);
        let cont = b"HTTP/1.1 100 Continue\r\n\r\n";
        assert_eq!(&mock.write[..cont.len()], cont);
        let res = b"HTTP/1.1 200 OK\r\n";
        assert_eq!(&mock.write[cont.len()..cont.len() + res.len()], res);
    }

    #[test]
    fn test_check_continue_reject() {
        struct Reject;
        impl Handler for Reject {
            fn handle(&self, _: Request, res: Response<Fresh>) {
                res.start().unwrap().end().unwrap();
            }

            fn check_continue(&self, _: (&Method, &RequestUri, &Headers)) -> StatusCode {
                StatusCode::ExpectationFailed
            }
        }

        let mut mock = MockStream::with_input(b"\
            POST /upload HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Expect: 100-continue\r\n\
            Content-Length: 10\r\n\
            \r\n\
            1234567890\
        ");

        Worker::new(Reject, Default::default()).handle_connection(&mut mock);
        assert_eq!(mock.write, &b"HTTP/1.1 417 Expectation Failed\r\n\r\n"[..]);
    }
    */
}
