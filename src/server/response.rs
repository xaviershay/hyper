//! Server Responses
//!
//! These are responses sent by a `hyper::Server` to clients, after
//! receiving a request.
use header;
use http;
use status;
use version;


/// The outgoing half for a Tcp connection, created by a `Server` and given to a `Handler`.
///
/// The default `StatusCode` for a `Response` is `200 OK`.
#[derive(Debug)]
pub struct Response<'a> {
    head: &'a mut http::ResponseHead
}

impl<'a> Response<'a> {
    /// The headers of this response.
    #[inline]
    pub fn headers(&self) -> &header::Headers { &self.head.headers }

    /// The status of this response.
    #[inline]
    pub fn status(&self) -> status::StatusCode { 
        status::StatusCode::from_u16(self.head.subject.0)
    }

    /// The HTTP version of this response.
    #[inline]
    pub fn version(&self) -> &version::HttpVersion { &self.head.version }

    /// Get a mutable reference to the Headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut header::Headers { &mut self.head.headers }

    /// Get a mutable reference to the status.
    #[inline]
    pub fn set_status(&mut self, status: status::StatusCode) {
        self.head.subject = status.into();
    }
}

/// Creates a new Response that can be used to write to a network stream.
pub fn new<'a>(head: &'a mut http::ResponseHead) -> Response<'a> {
    Response {
        head: head
    }
}

#[cfg(test)]
mod tests {
    /*
    use header::Headers;
    use mock::MockStream;
    use super::Response;

    macro_rules! lines {
        ($s:ident = $($line:pat),+) => ({
            let s = String::from_utf8($s.write).unwrap();
            let mut lines = s.split_terminator("\r\n");

            $(
                match lines.next() {
                    Some($line) => (),
                    other => panic!("line mismatch: {:?} != {:?}", other, stringify!($line))
                }
            )+

            assert_eq!(lines.next(), None);
        })
    }

    #[test]
    fn test_fresh_start() {
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let res = Response::new(&mut stream, &mut headers);
            res.start().unwrap().deconstruct();
        }

        lines! { stream =
            "HTTP/1.1 200 OK",
            _date,
            _transfer_encoding,
            ""
        }
    }

    #[test]
    fn test_streaming_end() {
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let res = Response::new(&mut stream, &mut headers);
            res.start().unwrap().end().unwrap();
        }

        lines! { stream =
            "HTTP/1.1 200 OK",
            _date,
            _transfer_encoding,
            "",
            "0",
            "" // empty zero body
        }
    }

    #[test]
    fn test_fresh_drop() {
        use status::StatusCode;
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let mut res = Response::new(&mut stream, &mut headers);
            *res.status_mut() = StatusCode::NotFound;
        }

        lines! { stream =
            "HTTP/1.1 404 Not Found",
            _date,
            _transfer_encoding,
            "",
            "0",
            "" // empty zero body
        }
    }

    // x86 windows msvc does not support unwinding
    // See https://github.com/rust-lang/rust/issues/25869
    #[cfg(not(all(windows, target_arch="x86", target_env="msvc")))]
    #[test]
    fn test_fresh_drop_panicing() {
        use std::thread;
        use std::sync::{Arc, Mutex};

        use status::StatusCode;

        let stream = MockStream::new();
        let stream = Arc::new(Mutex::new(stream));
        let inner_stream = stream.clone();
        let join_handle = thread::spawn(move || {
            let mut headers = Headers::new();
            let mut stream = inner_stream.lock().unwrap();
            let mut res = Response::new(&mut *stream, &mut headers);
            *res.status_mut() = StatusCode::NotFound;

            panic!("inside")
        });

        assert!(join_handle.join().is_err());

        let stream = match stream.lock() {
            Err(poisoned) => poisoned.into_inner().clone(),
            Ok(_) => unreachable!()
        };

        lines! { stream =
            "HTTP/1.1 500 Internal Server Error",
            _date,
            _transfer_encoding,
            "",
            "0",
            "" // empty zero body
        }
    }


    #[test]
    fn test_streaming_drop() {
        use std::io::Write;
        use status::StatusCode;
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let mut res = Response::new(&mut stream, &mut headers);
            *res.status_mut() = StatusCode::NotFound;
            let mut stream = res.start().unwrap();
            stream.write_all(b"foo").unwrap();
        }

        lines! { stream =
            "HTTP/1.1 404 Not Found",
            _date,
            _transfer_encoding,
            "",
            "3",
            "foo",
            "0",
            "" // empty zero body
        }
    }
    */
}
