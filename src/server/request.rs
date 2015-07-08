//! Server Requests
//!
//! These are requests that a `hyper::Server` receives, and include its method,
//! target URI, headers, and message body.
//use std::net::SocketAddr;
use std::fmt;

//use eventual::Future;

use version::HttpVersion;
use method::Method;
use header::Headers;
use http::{RequestHead, MessageHead, IncomingStream};
use uri::RequestUri;

pub fn new(incoming: RequestHead, stream: IncomingStream) -> Request {
    let MessageHead { version, subject: (method, uri), headers } = incoming;
    debug!("Request Line: {:?} {:?} {:?}", method, uri, version);
    debug!("{:#?}", headers);

    Request {
        //remote_addr: addr,
        method: method,
        uri: uri,
        headers: headers,
        version: version,
        body: stream,
    }
}

/// A request bundles several parts of an incoming `NetworkStream`, given to a `Handler`.
pub struct Request {
    // The IP address of the remote connection.
    //remote_addr: SocketAddr,
    method: Method,
    headers: Headers,
    uri: RequestUri,
    version: HttpVersion,
    body: IncomingStream,
}


impl Request {
    /// The `Method`, such as `Get`, `Post`, etc.
    #[inline]
    pub fn method(&self) -> &Method { &self.method }

    /// The headers of the incoming request.
    #[inline]
    pub fn headers(&self) -> &Headers { &self.headers }

    /// The target request-uri for this request.
    #[inline]
    pub fn uri(&self) -> &RequestUri { &self.uri }

    /// The version of HTTP for this request.
    #[inline]
    pub fn version(&self) -> &HttpVersion { &self.version }

    /*
    pub fn path(&self) -> Option<&str> {
        match *self.uri {
            RequestUri::AbsolutePath(ref s) => Some(s),
            RequestUri::AbsoluteUri(ref url) => (),
            _ => None
        }
    }
    */

    pub fn on_read<T: ::http::Read + Send + 'static>(self, callback: T) {
        self.body.read(callback);
    }

    pub fn read<F>(self, callback: F) where F: FnOnce(::Result<(&[u8], Self)>) + Send + 'static {
        let stream = self.body.clone();
        stream.read(::http::events::ReadOnce::new(move |result| {
            callback(result.map(move |data| (data, self)))
        }));
    }

    //pub fn read(mut self, read: R) {}
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("uri", &self.uri)
            .field("version", &self.version)
            .field("headers", &self.headers)
            .finish()
    }
}

#[cfg(test)]
mod tests {

    /*
    use buffer::BufReader;
    use header::{Host, TransferEncoding, Encoding};
    use net::NetworkStream;
    use mock::MockStream;
    use super::Request;

    use std::io::{self, Read};
    use std::net::SocketAddr;

    fn sock(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn read_to_string(mut req: Request) -> io::Result<String> {
        let mut s = String::new();
        try!(req.read_to_string(&mut s));
        Ok(s)
    }

    #[test]
    fn test_get_empty_body() {
        let mut mock = MockStream::with_input(b"\
            GET / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            \r\n\
            I'm a bad request.\r\n\
        ");

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();
        assert_eq!(read_to_string(req).unwrap(), "".to_owned());
    }

    #[test]
    fn test_head_empty_body() {
        let mut mock = MockStream::with_input(b"\
            HEAD / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            \r\n\
            I'm a bad request.\r\n\
        ");

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();
        assert_eq!(read_to_string(req).unwrap(), "".to_owned());
    }

    #[test]
    fn test_post_empty_body() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            \r\n\
            I'm a bad request.\r\n\
        ");

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();
        assert_eq!(read_to_string(req).unwrap(), "".to_owned());
    }

    #[test]
    fn test_parse_chunked_request() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            1\r\n\
            q\r\n\
            2\r\n\
            we\r\n\
            2\r\n\
            rt\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        // The headers are correct?
        match req.headers.get::<Host>() {
            Some(host) => {
                assert_eq!("example.domain", host.hostname);
            },
            None => panic!("Host header expected!"),
        };
        match req.headers.get::<TransferEncoding>() {
            Some(encodings) => {
                assert_eq!(1, encodings.len());
                assert_eq!(Encoding::Chunked, encodings[0]);
            }
            None => panic!("Transfer-Encoding: chunked expected!"),
        };
        // The content is correctly read?
        assert_eq!(read_to_string(req).unwrap(), "qwert".to_owned());
    }

    /// Tests that when a chunk size is not a valid radix-16 number, an error
    /// is returned.
    #[test]
    fn test_invalid_chunk_size_not_hex_digit() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            X\r\n\
            1\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        assert!(read_to_string(req).is_err());
    }

    /// Tests that when a chunk size contains an invalid extension, an error is
    /// returned.
    #[test]
    fn test_invalid_chunk_size_extension() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            1 this is an invalid extension\r\n\
            1\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        assert!(read_to_string(req).is_err());
    }

    /// Tests that when a valid extension that contains a digit is appended to
    /// the chunk size, the chunk is correctly read.
    #[test]
    fn test_chunk_size_with_extension() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            1;this is an extension with a digit 1\r\n\
            1\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        assert_eq!(read_to_string(req).unwrap(), "1".to_owned());
    }*/

}
