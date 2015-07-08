use std::fmt;
use std::io::{self, Write};
use std::marker::PhantomData;
use std::sync::mpsc;

use url::Url;
use tick::{self, Interest};
use time::now_utc;

use header::{self, Headers};
use http::{self, events, conn};
use method::Method;
use net::{Fresh, Streaming};
use status::StatusCode;
use version::HttpVersion;

pub use self::decode::Decoder;
use self::encode::Encoder;

pub use self::parse::parse;

mod decode;
mod encode;
mod parse;

/*
fn should_have_response_body(method: &Method, status: u16) -> bool {
    trace!("should_have_response_body({:?}, {})", method, status);
    match (method, status) {
        (&Method::Head, _) |
        (_, 100...199) |
        (_, 204) |
        (_, 304) |
        (&Method::Connect, 200...299) => false,
        _ => true
    }
}
*/

#[derive(Clone)]
pub struct IncomingStream {
    on_read: mpsc::Sender<ReadCb>,
    transfer: tick::Transfer,
}

pub struct Incoming {
    decoder: Decoder,
    state: ReadState,
    want_keep_alive: bool,
    on_read: mpsc::Receiver<ReadCb>,
}

type ReadCb = Box<events::Read + Send + 'static>;
type ReadState = EventState<ReadCb>;

pub fn incoming(decoder: Decoder, transfer: tick::Transfer, keep_alive: bool) -> (IncomingStream, Incoming) {
    let (tx, rx) = mpsc::channel();
    (IncomingStream {
        on_read: tx,
        transfer: transfer,
    }, Incoming {
        decoder: decoder,
        state: EventState::Paused,
        want_keep_alive: keep_alive,
        on_read: rx
    })
}

impl IncomingStream {

    pub fn read<E: events::Read + Send + 'static>(mut self, on_read: E) {
        self.set_read(Box::new(on_read));
    }

    fn set_read(&mut self, on_read: Box<events::Read + Send + 'static>) {
        self.on_read.send(on_read)
            .expect("Receiver should never drop before Sender");
        self.transfer.interest(tick::Interest::Read);
    }
}

impl Incoming {
    pub fn on_read<R: io::Read>(&mut self, transport: &mut R) -> io::Result<()> {
        self.check_state();
        let state = &mut self.state;
        match *state {
            EventState::Ready(ref mut on) => try!(on.on_read(&mut DecoderReader {
                decoder: &mut self.decoder,
                transport: transport,
            })),
            _ => return Ok(())
        }
        *state = if self.decoder.is_eof() {
            EventState::Eof
        } else {
            EventState::Paused
        };
        Ok(())
    }

    pub fn next(&mut self) -> conn::Next {
        self.check_state();
        match self.state {
            EventState::Ready(..) => conn::Next::Continue,
            EventState::Paused => conn::Next::Pause,
            EventState::Eof => conn::Next::Eof,
        }
    }

    pub fn keep_alive(&self) -> bool {
        self.want_keep_alive && self.decoder.is_eof()
    }

    fn check_state(&mut self) {
        // should only look for new states if paused
        match self.state {
            EventState::Paused => (),
            _ => return
        }
        loop {
            match self.on_read.try_recv() {
                Ok(on_read) => self.state = EventState::Ready(on_read),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state = EventState::Eof;
                    break
                }
            }
        }
    }
}

/// The part of an OutgoingStream that is kept in the event loop.
pub struct Outgoing {
    encoder: Encoder,
    state: WriteState,
    want_keep_alive: bool,
    on_write: mpsc::Receiver<WriteMsg>
}

pub struct OutgoingStream<T, S> {
    on_write: mpsc::Sender<WriteMsg>,
    transfer: tick::Transfer,
    _type: PhantomData<T>,
    _state: PhantomData<S>,
}

impl<T, S> Clone for OutgoingStream<T, S> {
    fn clone(&self) -> OutgoingStream<T, S> {
        OutgoingStream {
            on_write: self.on_write.clone(),
            transfer: self.transfer.clone(),
            _type: PhantomData,
            _state: PhantomData,
        }
    }
}

type WriteCb = Box<events::Write + Send + 'static>;
type WriteState = EventState<WriteMsg>;
struct WriteMsg {
    callback: WriteCb,
    encoder: Option<Encoder>,
}

pub fn outgoing<T, S>(transfer: tick::Transfer, keep_alive: bool) -> (OutgoingStream<T, S>, Outgoing) {
    let (tx, rx) = mpsc::channel();
    (
        OutgoingStream {
            on_write: tx,
            transfer: transfer,
            _type: PhantomData,
            _state: PhantomData,
        },
        Outgoing {
            encoder: Encoder::Through,
            on_write: rx,
            want_keep_alive: keep_alive,
            state: EventState::Paused,
        }
    )
}

const AVERAGE_HEADER_SIZE: usize = 100;

impl OutgoingStream<http::Response, Fresh> {
    pub fn start<F>(mut self, version: HttpVersion, status: StatusCode, mut headers: Headers, callback: F)
    where F: FnOnce(::Result<(HttpVersion, StatusCode, Headers, OutgoingStream<http::Response, Streaming>)>) + Send + 'static {
        debug!("writing head: {:?} {:?}", version, status);
        let mut buf = Vec::with_capacity(30 + headers.len() * AVERAGE_HEADER_SIZE);
        let _ = write!(&mut buf, "{} {}\r\n", version, status);

        if !headers.has::<header::Date>() {
            headers.set(header::Date(header::HttpDate(now_utc())));
        }

        let mut body = Body::Chunked;
        if let Some(cl) = headers.get::<header::ContentLength>() {
            body = Body::Sized(**cl);
        }

        if body == Body::Chunked {
            let encodings = match headers.get_mut::<header::TransferEncoding>() {
                Some(&mut header::TransferEncoding(ref mut encodings)) => {
                    //TODO: check if chunked is already in encodings. use HashSet?
                    encodings.push(header::Encoding::Chunked);
                    false
                },
                None => true
            };

            if encodings {
                headers.set(header::TransferEncoding(vec![header::Encoding::Chunked]));
            }
            body = Body::Chunked;
        }


        debug!("{:#?}", headers);
        let _ = write!(&mut buf, "{}\r\n", headers);

        let encoder = match body {
            Body::Sized(len) => Encoder::Length(len),
            Body::Chunked => Encoder::Chunked
        };

        let on_write = self.on_write.clone();
        let transfer = self.transfer.clone();
        let cb = Box::new(events::WriteAll::new(buf, move |result| {
            callback(result.map(move |_| (
                version,
                status,
                headers,
                OutgoingStream {
                    on_write: on_write,
                    transfer: transfer,
                    _type: PhantomData,
                    _state: PhantomData,
                }
            )));
        }));
        self.set_write(WriteMsg {
            callback: cb,
            encoder: Some(encoder)
        });
    }
}

impl OutgoingStream<http::Request, Fresh> {
    pub fn start<F>(mut self, method: Method, url: Url, mut headers: Headers, callback: F)
    where F: FnOnce(::Result<(Method, Url, Headers, OutgoingStream<http::Request, Streaming>)>) + Send + 'static {
        let mut uri = url.serialize_path().unwrap();
        if let Some(ref q) = url.query {
            uri.push('?');
            uri.push_str(&q[..]);
        }

        let version = HttpVersion::Http11;
        let mut buf = Vec::with_capacity(30 + headers.len() * AVERAGE_HEADER_SIZE);
        debug!("request line: {:?} {:?} {:?}", method, uri, version);
        let _ = write!(&mut buf, "{} {} {}\r\n", method, uri, version);

        debug!("{:#?}", headers);
        let encoder = match &method {
            &Method::Get | &Method::Head => {
                let _ = write!(&mut buf, "{}\r\n", headers);
                Encoder::Length(0)
            },
            _ => {
                let mut chunked = true;
                let mut len = 0;

                match headers.get::<header::ContentLength>() {
                    Some(cl) => {
                        chunked = false;
                        len = **cl;
                    },
                    None => ()
                };

                // can't do in match above, thanks borrowck
                if chunked {
                    let encodings = match headers.get_mut::<header::TransferEncoding>() {
                        Some(encodings) => {
                            //TODO: check if chunked is already in encodings. use HashSet?
                            encodings.push(header::Encoding::Chunked);
                            false
                        },
                        None => true
                    };

                    if encodings {
                        headers.set(
                            header::TransferEncoding(vec![header::Encoding::Chunked]))
                    }
                }

                let _ = write!(&mut buf, "{}\r\n", headers);

                if chunked {
                    Encoder::Chunked
                } else {
                    Encoder::Length(len)
                }
            }
        };

        let on_write = self.on_write.clone();
        let transfer = self.transfer.clone();

        let cb = Box::new(events::WriteAll::new(buf, move |result| {
            callback(result.map(move |_| (
                method,
                url,
                headers,
                OutgoingStream {
                    on_write: on_write,
                    transfer: transfer,
                    _type: PhantomData,
                    _state: PhantomData,
                }
            )));
        }));

        self.set_write(WriteMsg {
            callback: cb,
            encoder: Some(encoder),
        });
    }
}

impl<T> OutgoingStream<T, Streaming> {
    #[inline]
    pub fn write<E: events::Write + Send + 'static>(mut self, on_write: E) {
        self.set_write(WriteMsg {
            callback: Box::new(on_write),
            encoder: None 
        });
    }
}

impl<T, S> OutgoingStream<T, S> {
    fn set_write(&mut self, on_write: WriteMsg) {
        self.on_write.send(on_write)
            .expect("Receiver should never drop before Sender");
        self.transfer.interest(tick::Interest::Write);
    }
}

impl Outgoing {
    pub fn on_write<W: io::Write>(&mut self, transport: &mut W) -> io::Result<()> {
        loop {
            self.check_state();
            let state = &mut self.state;
            match *state {
                EventState::Ready(ref mut msg) => {
                    try!(msg.callback.on_write(&mut EncoderWriter {
                        encoder: &mut self.encoder,
                        transport: transport,
                    }));
                    if let Some(encoder) = msg.encoder.take() {
                        trace!("updating encoder to {:?}", encoder);
                        self.encoder = encoder;
                    }
                }
                EventState::Paused | EventState::Eof => return Ok(())
            }
            *state = if self.encoder.is_eof() {
                EventState::Eof
            } else {
                 EventState::Paused
            };
        }
    }

    pub fn keep_alive(&self) -> bool {
        self.want_keep_alive && self.encoder.is_eof()
    }

    pub fn next(&mut self) -> conn::Next {
        self.check_state();
        match self.state {
            EventState::Ready(..) => conn::Next::Continue,
            EventState::Paused => conn::Next::Pause,
            EventState::Eof => conn::Next::Eof
        }
    }

    fn check_state(&mut self) {
        // should only look for new states if paused
        match self.state {
            EventState::Paused => (),
            _ => return
        }
        loop {
            match self.on_write.try_recv() {
                Ok(on_write) => self.state = EventState::Ready(on_write),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state = EventState::Eof;
                    break
                }
            }
        }
    }
}

struct DecoderReader<'a> {
    decoder: &'a mut Decoder,
    transport: &'a mut io::Read,
}

impl<'a> io::Read for DecoderReader<'a> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.decoder.decode(&mut self.transport, buf)
    }
}

struct EncoderWriter<'a> {
    encoder: &'a mut Encoder,
    transport: &'a mut io::Write
}

impl<'a> io::Write for EncoderWriter<'a> {
    #[inline]
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.encoder.encode(&mut self.transport, data)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.transport.flush()
    }
}

#[derive(PartialEq, Debug)]
enum Body {
    Sized(u64),
    Chunked
}

enum EventState<T> {
    Ready(T),
    Eof,
    Paused
}

impl<T> fmt::Debug for EventState<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            EventState::Ready(..) => f.write_str("Ready(..)"),
            EventState::Eof => f.write_str("Eof"),
            EventState::Paused => f.write_str("Paused"),

        }
    }
}

/*
const MAX_INVALID_RESPONSE_BYTES: usize = 1024 * 128;
impl HttpMessage for Http11Message {

    fn get_incoming(&mut self) -> ::Result<ResponseHead> {
        unimplemented!();
        /*
        try!(self.flush_outgoing());
        let stream = match self.stream.take() {
            Some(stream) => stream,
            None => {
                // The message was already in the reading state...
                // TODO Decide what happens in case we try to get a new incoming at that point
                return Err(From::from(
                        io::Error::new(io::ErrorKind::Other,
                        "Read already in progress")));
            }
        };

        let expected_no_content = stream.previous_response_expected_no_content();
        trace!("previous_response_expected_no_content = {}", expected_no_content);

        let mut stream = BufReader::new(stream);

        let mut invalid_bytes_read = 0;
        let head;
        loop {
            head = match parse_response(&mut stream) {
                Ok(head) => head,
                Err(::Error::Version)
                if expected_no_content && invalid_bytes_read < MAX_INVALID_RESPONSE_BYTES => {
                    trace!("expected_no_content, found content");
                    invalid_bytes_read += 1;
                    stream.consume(1);
                    continue;
                }
                Err(e) => {
                    self.stream = Some(stream.into_inner());
                    return Err(e);
                }
            };
            break;
        }

        let raw_status = head.subject;
        let headers = head.headers;

        let method = self.method.take().unwrap_or(Method::Get);

        let is_empty = !should_have_response_body(&method, raw_status.0);
        stream.get_mut().set_previous_response_expected_no_content(is_empty);
        // According to https://tools.ietf.org/html/rfc7230#section-3.3.3
        // 1. HEAD reponses, and Status 1xx, 204, and 304 cannot have a body.
        // 2. Status 2xx to a CONNECT cannot have a body.
        // 3. Transfer-Encoding: chunked has a chunked body.
        // 4. If multiple differing Content-Length headers or invalid, close connection.
        // 5. Content-Length header has a sized body.
        // 6. Not Client.
        // 7. Read till EOF.
        self.reader = Some(if is_empty {
            SizedReader(stream, 0)
        } else {
             if let Some(&TransferEncoding(ref codings)) = headers.get() {
                if codings.last() == Some(&Chunked) {
                    ChunkedReader(stream, None)
                } else {
                    trace!("not chuncked. read till eof");
                    EofReader(stream)
                }
            } else if let Some(&ContentLength(len)) =  headers.get() {
                SizedReader(stream, len)
            } else if headers.has::<ContentLength>() {
                trace!("illegal Content-Length: {:?}", headers.get_raw("Content-Length"));
                return Err(Error::Header);
            } else {
                trace!("neither Transfer-Encoding nor Content-Length");
                EofReader(stream)
            }
        });

        trace!("Http11Message.reader = {:?}", self.reader);


        Ok(ResponseHead {
            headers: headers,
            raw_status: raw_status,
            version: head.version,
        })
        */
    }
}


*/


