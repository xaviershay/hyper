//! Server Responses
//!
//! These are responses sent by a `hyper::Server` to clients, after
//! receiving a request.
use std::any::Any;
use std::fmt;
use std::mem;
use std::ptr;
use std::thread;

use header;
use http;
use status;
use net::{Fresh, Streaming};
use version;


/// The outgoing half for a Tcp connection, created by a `Server` and given to a `Handler`.
///
/// The default `StatusCode` for a `Response` is `200 OK`.
///
/// There is a `Drop` implementation for `Response` that will automatically
/// write the head and flush the body, if the handler has not already done so,
/// so that the server doesn't accidentally leave dangling requests.
pub struct Response<W: Any = Fresh> {
    inner: Inner<W>,
}

struct Inner<W> {
    version: version::HttpVersion,
    // The status code for the request.
    status: status::StatusCode,
    // The outgoing headers on this response.
    headers: header::Headers,
    stream: http::OutgoingStream<http::Response, W>,
}

impl<W: Any> Response<W> {

    /// The headers of this response.
    #[inline]
    pub fn headers(&self) -> &header::Headers { &self.inner.headers }

    /// The status of this response.
    #[inline]
    pub fn status(&self) -> status::StatusCode { self.inner.status }

    /// The HTTP version of this response.
    #[inline]
    pub fn version(&self) -> &version::HttpVersion { &self.inner.version }


    /*
    /// Construct a Response from its constituent parts.
    #[inline]
    pub fn construct(version: version::HttpVersion,
                     body: HttpWriter<&'a mut (Write + 'a)>,
                     status: status::StatusCode,
                     headers: &'a mut header::Headers) -> Response<'a, Fresh> {
        Response {
            status: status,
            version: version,
            body: body,
            headers: headers,
        }
    }
    */

    fn deconstruct(self) -> Inner<W> {
        unsafe {
            let inner = ptr::read(&self.inner);
            mem::forget(self);
            inner
        }
    }
}

/// Creates a new Response that can be used to write to a network stream.
pub fn new(tx: http::OutgoingStream<http::Response, Fresh>) -> Response<Fresh> {
    Response {
        inner: Inner {
            status: status::StatusCode::Ok,
            version: version::HttpVersion::Http11,
            headers: header::Headers::new(),
            stream: tx,
        },
    }
}

impl Response<Fresh> {
    /// Get a mutable reference to the Headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut header::Headers { &mut self.inner.headers }

    /// Get a mutable reference to the status.
    #[inline]
    pub fn status_mut(&mut self) -> &mut status::StatusCode { &mut self.inner.status }

    pub fn start<F>(self, callback: F) where F: FnOnce(::Result<Response<Streaming>>) + Send + 'static {
        let inner = self.deconstruct();
        inner.stream.start(inner.version, inner.status, inner.headers, move |result| {
            callback(result.map(|(version, status, headers, stream)| Response {
                inner: Inner {
                    status: status,
                    version: version,
                    headers: headers,
                    stream: stream
                },
            }));
        });
    }
    /// Writes the body and ends the response.
    ///
    /// This is a shortcut method for when you have a response with a fixed
    /// size, and would only need a single `write` call normally.
    ///
    /// # Example
    ///
    /// ```
    /// # use hyper::server::Response;
    /// fn hello_world(res: Response) {
    ///     res.send(b"Hello World!")
    /// }
    /// ```
    ///
    /// The above is a short for this longer form:
    ///
    /// ```
    /// # use hyper::server::Response;
    /// use std::io::Write;
    /// use hyper::header::ContentLength;
    /// fn handler(mut res: Response) {
    ///     let body = b"Hello World!";
    ///     res.headers_mut().set(ContentLength(body.len() as u64));
    ///     res.start().write(body);
    /// }
    /// ```
    #[inline]
    pub fn send<T>(mut self, data: T) where T: AsRef<[u8]> + Send + 'static {
        self.inner.headers.set(header::ContentLength(data.as_ref().len() as u64));
        self.start(move |result| {
            trace!("send on_complete");
            match result {
                Ok(streaming) => streaming.write_all(data, |_| ()),
                Err(e) => error!("error starting request: {:?}", e)
            }
        });
    }

    /*
    /// Consume this Response<Fresh>, writing the Headers and Status and
    /// creating a Response<Streaming>
    pub fn start(self) -> Response<Streaming> {
        let (version, body, status, mut headers) = self.deconstruct();
        let body = body.start(version, status, &mut headers);
        Response {
            version: version,
            status: status,
            headers: headers,
            body: body
        }
    }
    */
}

impl Response<Streaming> {
    pub fn write_all<T, F>(self, data: T, callback: F)
    where T: AsRef<[u8]> + Send + 'static, F: FnOnce(::Result<Response<Streaming>>) + Send + 'static {
        let stream = self.inner.stream.clone();
        stream.write(::http::events::WriteAll::new(data, move |result| {
            callback(result.map(move |_| self))
        }));
    }
    /*
    /// Asynchronously write bytes to the response.
    #[inline]
    pub fn write(&mut self, data: &[u8]) {
        self.stream.write(data)
    }
    */

    //pub fn drain(&mut self, callback: F) -> Future {}

}

impl<T: Any> Drop for Response<T> {
    fn drop(&mut self) {
        use std::any::TypeId;
        if TypeId::of::<T>() == TypeId::of::<Fresh>() {
            if thread::panicking() {
                self.status = status::StatusCode::InternalServerError;
            }
            let me: &mut Response<Fresh> = unsafe { mem::transmute(self) };
            me.inner.headers.set(header::ContentLength(0));
            let headers = mem::replace(&mut me.inner.headers, header::Headers::new());
            let body = me.inner.stream.clone();
            body.start(me.inner.version, me.inner.status, headers, |_| ());
        }

        /*
        //TODO: this should happen in http::OutgoingStream
        // AsyncWriter will flush on drop
        if !http::should_keep_alive(self.version, &self.headers) {
            trace!("not keep alive, closing");
            self.body.get_mut().get_mut().get_mut().close();
        }
        */
    }
}

impl<T: Any> fmt::Debug for Response<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.inner.status)
            .field("version", &self.inner.version)
            .field("headers", &self.inner.headers)
            .finish()
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
