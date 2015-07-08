//! Pieces pertaining to the HTTP message protocol.
use std::borrow::Cow;

use header::Connection;
use header::ConnectionOption::{KeepAlive, Close};
use header::Headers;
use method::Method;
use uri::RequestUri;
use version::HttpVersion;
use version::HttpVersion::{Http10, Http11};

#[cfg(feature = "serde-serialization")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use self::conn::{Conn, Handler};
pub use self::events::{Read, Write, Data};

mod buffer;
pub mod conn;
pub mod events;
pub mod h1;
//pub mod h2;

// pub enum OutgoingStream { Http11(h1::OutgoingStream), Http2(h2::Outgoing) }
pub use self::h1::IncomingStream;
pub use self::h1::OutgoingStream;
pub use self::h1::Decoder;

/// Marker used with Streams to define semantics.
#[derive(Debug)]
pub enum Request {}
/// Marker used with Streams to define semantics.
#[derive(Debug)]
pub enum Response {}

/// An Incoming Message head. Includes request/status line, and headers.
#[derive(Debug)]
pub struct MessageHead<S> {
    /// HTTP version of the message.
    pub version: HttpVersion,
    /// Subject (request line or status line) of Incoming message.
    pub subject: S,
    /// Headers of the Incoming message.
    pub headers: Headers
}

/// An incoming request message.
pub type RequestHead = MessageHead<(Method, RequestUri)>;

/// An incoming response message.
pub type ResponseHead = MessageHead<RawStatus>;

impl<S> MessageHead<S> {
    pub fn should_keep_alive(&self) -> bool {
        should_keep_alive(self.version, &self.headers)
    }
}

/// The raw status code and reason-phrase.
#[derive(Clone, PartialEq, Debug)]
pub struct RawStatus(pub u16, pub Cow<'static, str>);

#[cfg(feature = "serde-serialization")]
impl Serialize for RawStatus {
    fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error> where S: Serializer {
        (self.0, &self.1).serialize(serializer)
    }
}

#[cfg(feature = "serde-serialization")]
impl Deserialize for RawStatus {
    fn deserialize<D>(deserializer: &mut D) -> Result<RawStatus, D::Error> where D: Deserializer {
        let representation: (u16, String) = try!(Deserialize::deserialize(deserializer));
        Ok(RawStatus(representation.0, Cow::Owned(representation.1)))
    }
}

/// Checks if a connection should be kept alive.
#[inline]
pub fn should_keep_alive(version: HttpVersion, headers: &Headers) -> bool {
    trace!("should_keep_alive( {:?}, {:?} )", version, headers.get::<Connection>());
    match (version, headers.get::<Connection>()) {
        (Http10, None) => false,
        (Http10, Some(conn)) if !conn.contains(&KeepAlive) => false,
        (Http11, Some(conn)) if conn.contains(&Close)  => false,
        _ => true
    }
}

pub trait Parse {
    type Subject;
    fn parse(bytes: &[u8]) -> ParseResult<Self::Subject>;
    fn decoder(head: &MessageHead<Self::Subject>) -> ::Result<Decoder>;
}

pub type ParseResult<T> = ::Result<Option<(MessageHead<T>, usize)>>;

pub fn parse<T: Parse<Subject=I>, I>(rdr: &[u8]) -> ParseResult<I> {
    h1::parse::<T, I>(rdr)
}

#[test]
fn test_should_keep_alive() {
    let mut headers = Headers::new();

    assert!(!should_keep_alive(Http10, &headers));
    assert!(should_keep_alive(Http11, &headers));

    headers.set(Connection::close());
    assert!(!should_keep_alive(Http10, &headers));
    assert!(!should_keep_alive(Http11, &headers));

    headers.set(Connection::keep_alive());
    assert!(should_keep_alive(Http10, &headers));
    assert!(should_keep_alive(Http11, &headers));
}
