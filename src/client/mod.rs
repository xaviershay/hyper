//! HTTP Client
//!
//! # Usage
//!
//! The `Client` API is designed for most people to make HTTP requests.
//! It utilizes the lower level `Request` API.
//!
//! ## GET
//!
//! ```no_run
//! # use hyper::Client;
//! let client = Client::new();
//!
//! let res = client.get("http://example.domain").send().unwrap();
//! assert_eq!(res.status, hyper::Ok);
//! ```
//!
//! The returned value is a `Response`, which provides easy access to
//! the `status`, the `headers`, and the response body via the `Read`
//! trait.
//!
//! ## POST
//!
//! ```no_run
//! # use hyper::Client;
//! let client = Client::new();
//!
//! let res = client.post("http://example.domain")
//!     .body("foo=bar")
//!     .send()
//!     .unwrap();
//! assert_eq!(res.status, hyper::Ok);
//! ```
//!
//! # Sync
//!
//! The `Client` implements `Sync`, so you can share it among multiple threads
//! and make multiple requests simultaneously.
//!
//! ```no_run
//! # use hyper::Client;
//! use std::sync::Arc;
//! use std::thread;
//!
//! // Note: an Arc is used here because `thread::spawn` creates threads that
//! // can outlive the main thread, so we must use reference counting to keep
//! // the Client alive long enough. Scoped threads could skip the Arc.
//! let client = Arc::new(Client::new());
//! let clone1 = client.clone();
//! let clone2 = client.clone();
//! thread::spawn(move || {
//!     clone1.get("http://example.domain").send().unwrap();
//! });
//! thread::spawn(move || {
//!     clone2.post("http://example.domain/post").body("foo=bar").send().unwrap();
//! });
//! ```
use std::default::Default;
use std::io::{self, copy, Read};
use std::iter::Extend;

use tick;

use url::UrlParser;
use url::ParseError as UrlError;

use header::{Headers, Header, HeaderFormat};
use header::{ContentLength, Location};
use http;
use method::Method;
use net::{Transport, NetworkConnector, DefaultConnector};
use {Url};
use Error;

//use self::pool::Pool;
pub use self::request::Request;
pub use self::response::Response;

//mod pool;
mod request;
mod response;


/// A Client to use additional features with Requests.
///
/// Clients can handle things such as: redirect policy, connection pooling.
pub struct Client<C: NetworkConnector = DefaultConnector> {
    connector: C,
    //tick: tick::Tick<C::Stream, Factory>,
    redirect_policy: RedirectPolicy,
}

/*
pub struct ClientConfig {

}
*/

impl Client {
    /// Create a new Client.
    pub fn new() -> Client {
        Client::with_connector(DefaultConnector::default())
    }
}

impl<C: NetworkConnector> Client<C> {

    /// Create a new client with a specific connector.
    pub fn with_connector(connector: C) -> Client<C> {
        Client {
            connector: connector,
            //tick: tick::Tick::new(Factory),
            redirect_policy: RedirectPolicy::default(),
        }
    }

    /// Set the RedirectPolicy.
    pub fn set_redirect_policy(&mut self, policy: RedirectPolicy) {
        self.redirect_policy = policy;
    }

    /*
    /// Build a new request using this Client.
    pub fn request<U: IntoUrl>(&self, url: U, handler: H) {
        let url = url.into_url().unwrap()
    }

    fn stream(&self, transport: T, outgoing: (Method, Url, Headers)) {
        self.tick.stream(transport)
    }
    */
}

pub trait Handler<T: Transport>: Send + 'static {
    fn on_request(&mut self, request: &mut Request) -> http::Next;
    fn on_request_writable(&mut self, request: &mut http::Encoder<T>) -> http::Next;
    fn on_response(&mut self, response: Response) -> http::Next;
    fn on_response_readable(&mut self, response: &mut http::Decoder<T>) -> http::Next;
}

/*
struct Factory;

impl tick::ProtocolFactory for Factory {
    type Protocol = http::Conn<Handler>;
    fn create(&mut self, transfer: tick::Transfer, id: tick::Id) -> Self::Protocol {
        trace!("Factory.create {:?}", id);
        http::Conn::new(transfer, Handler)
    }
}
*/

struct Message<H: Handler<T>, T: Transport> {
    handler: H,
    _marker: PhantomData<T>,
}


impl<H: Handler<T>, T: Transport> http::MessageHandler for Message<H, T> {
    type Message = http::ClientMessage;

    fn on_outgoing(&mut self, head: &mut RequestHead) -> Next {
        let mut req = request::nead(head);
        self.handler.on_request(&mut req)
    }

    fn on_encode(&mut self, transport: &mut http::Encoder<T>) -> Next {
        self.handler.on_request_writable(transport)
    }

    fn on_incoming(&mut self, head: http::ResponseHead) -> Next {
        trace!("on_incoming {:?}", head);
        let resp = response::new(head);
        self.handler.on_response(resp)
    }

    fn on_decode(&mut self, transport: &mut http::Decoder<T>) -> Next {
        self.handler.on_response_readable(transport)
    }
}


    /*
    fn _send(self) -> ::Result<Response> {
        let mut url = try!(url.into_url());

        let can_have_body = match &method {
            &Method::Get | &Method::Head => false,
            _ => true
        };

        let mut body = if can_have_body {
            body
        } else {
            None
        };

        loop {
            let mut req = try!(Request::with_message(method.clone(), url.clone(), message));
            headers.as_ref().map(|headers| req.headers_mut().extend(headers.iter()));

            try!(req.set_write_timeout(client.write_timeout));
            try!(req.set_read_timeout(client.read_timeout));

            match (can_have_body, body.as_ref()) {
                (true, Some(body)) => match body.size() {
                    Some(size) => req.headers_mut().set(ContentLength(size)),
                    None => (), // chunked, Request will add it automatically
                },
                (true, None) => req.headers_mut().set(ContentLength(0)),
                _ => () // neither
            }
            let mut streaming = try!(req.start());
            body.take().map(|mut rdr| copy(&mut rdr, &mut streaming));
            let res = try!(streaming.send());
            if !res.status.is_redirection() {
                return Ok(res)
            }
            debug!("redirect code {:?} for {}", res.status, url);

            let loc = {
                // punching borrowck here
                let loc = match res.headers.get::<Location>() {
                    Some(&Location(ref loc)) => {
                        Some(UrlParser::new().base_url(&url).parse(&loc[..]))
                    }
                    None => {
                        debug!("no Location header");
                        // could be 304 Not Modified?
                        None
                    }
                };
                match loc {
                    Some(r) => r,
                    None => return Ok(res)
                }
            };
            url = match loc {
                Ok(u) => u,
                Err(e) => {
                    debug!("Location header had invalid URI: {:?}", e);
                    return Ok(res);
                }
            };
            match client.redirect_policy {
                // separate branches because they can't be one
                RedirectPolicy::FollowAll => (), //continue
                RedirectPolicy::FollowIf(cond) if cond(&url) => (), //continue
                _ => return Ok(res),
            }
        }
    }
    */

/// A helper trait to convert common objects into a Url.
pub trait IntoUrl {
    /// Consumes the object, trying to return a Url.
    fn into_url(self) -> Result<Url, UrlError>;
}

impl IntoUrl for Url {
    fn into_url(self) -> Result<Url, UrlError> {
        Ok(self)
    }
}

impl<'a> IntoUrl for &'a str {
    fn into_url(self) -> Result<Url, UrlError> {
        Url::parse(self)
    }
}

impl<'a> IntoUrl for &'a String {
    fn into_url(self) -> Result<Url, UrlError> {
        Url::parse(self)
    }
}

/// Behavior regarding how to handle redirects within a Client.
#[derive(Copy)]
pub enum RedirectPolicy {
    /// Don't follow any redirects.
    FollowNone,
    /// Follow all redirects.
    FollowAll,
    /// Follow a redirect if the contained function returns true.
    FollowIf(fn(&Url) -> bool),
}

// This is a hack because of upstream typesystem issues.
impl Clone for RedirectPolicy {
    fn clone(&self) -> RedirectPolicy {
        *self
    }
}

impl Default for RedirectPolicy {
    fn default() -> RedirectPolicy {
        RedirectPolicy::FollowAll
    }
}

fn get_host_and_port(url: &Url) -> ::Result<(String, u16)> {
    let host = match url.serialize_host() {
        Some(host) => host,
        None => return Err(Error::Uri(UrlError::EmptyHost))
    };
    trace!("host={:?}", host);
    let port = match url.port_or_default() {
        Some(port) => port,
        None => return Err(Error::Uri(UrlError::InvalidPort))
    };
    trace!("port={:?}", port);
    Ok((host, port))
}

#[cfg(test)]
mod tests {
    /*
    use std::io::Read;
    use header::Server;
    use super::{Client, RedirectPolicy};
    use super::pool::Pool;
    use url::Url;

    mock_connector!(MockRedirectPolicy {
        "http://127.0.0.1" =>       "HTTP/1.1 301 Redirect\r\n\
                                     Location: http://127.0.0.2\r\n\
                                     Server: mock1\r\n\
                                     \r\n\
                                    "
        "http://127.0.0.2" =>       "HTTP/1.1 302 Found\r\n\
                                     Location: https://127.0.0.3\r\n\
                                     Server: mock2\r\n\
                                     \r\n\
                                    "
        "https://127.0.0.3" =>      "HTTP/1.1 200 OK\r\n\
                                     Server: mock3\r\n\
                                     \r\n\
                                    "
    });

    #[test]
    fn test_redirect_followall() {
        let mut client = Client::with_connector(MockRedirectPolicy);
        client.set_redirect_policy(RedirectPolicy::FollowAll);

        let res = client.get("http://127.0.0.1").send().unwrap();
        assert_eq!(res.headers.get(), Some(&Server("mock3".to_owned())));
    }

    #[test]
    fn test_redirect_dontfollow() {
        let mut client = Client::with_connector(MockRedirectPolicy);
        client.set_redirect_policy(RedirectPolicy::FollowNone);
        let res = client.get("http://127.0.0.1").send().unwrap();
        assert_eq!(res.headers.get(), Some(&Server("mock1".to_owned())));
    }

    #[test]
    fn test_redirect_followif() {
        fn follow_if(url: &Url) -> bool {
            !url.serialize().contains("127.0.0.3")
        }
        let mut client = Client::with_connector(MockRedirectPolicy);
        client.set_redirect_policy(RedirectPolicy::FollowIf(follow_if));
        let res = client.get("http://127.0.0.1").send().unwrap();
        assert_eq!(res.headers.get(), Some(&Server("mock2".to_owned())));
    }

    mock_connector!(Issue640Connector {
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n",
        b"GET",
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n",
        b"HEAD",
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n",
        b"POST"
    });

    // see issue #640
    #[test]
    fn test_head_response_body_keep_alive() {
        let client = Client::with_connector(Pool::with_connector(Default::default(), Issue640Connector));

        let mut s = String::new();
        client.get("http://127.0.0.1").send().unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "GET");

        let mut s = String::new();
        client.head("http://127.0.0.1").send().unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "");

        let mut s = String::new();
        client.post("http://127.0.0.1").send().unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "POST");
    }
    */
}
