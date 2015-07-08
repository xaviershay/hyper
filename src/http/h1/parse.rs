use std::borrow::Cow;
use std::io;

use httparse;

use header::Headers;
use http::{MessageHead, RawStatus, Http1Message, ParseResult, Next, ServerMessage, ClientMessage};
use http::h1::{Encoder, Decoder};
use method::Method;
use status::StatusCode;
use uri::RequestUri;
use version::HttpVersion::{Http10, Http11};

const MAX_HEADERS: usize = 100;

/*
/// Parses a request into an Incoming message head.
#[inline]
pub fn parse_request(buf: &[u8]) -> ParseResult<(Method, RequestUri)> {
    parse::<httparse::Request, (Method, RequestUri)>(buf)
}

/// Parses a response into an Incoming message head.
#[inline]
pub fn parse_response(buf: &[u8]) -> ParseResult<RawStatus> {
    parse::<httparse::Response, RawStatus>(buf)
}
*/

pub fn parse<T: Http1Message<Incoming=I>, I>(buf: &[u8]) -> ParseResult<I> {
    if buf.len() == 0 {
        return Ok(None);
    }
    trace!("parse({:?})", buf);
    <T as Http1Message>::parse(buf)
}



impl Http1Message for ServerMessage {
    type Incoming = (Method, RequestUri);
    type Outgoing = RawStatus;

    fn initial_interest() -> Next {
        Next::read()
    }

    fn parse(buf: &[u8]) -> ParseResult<(Method, RequestUri)> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        trace!("Request.parse([Header; {}], [u8; {}])", headers.len(), buf.len());
        let mut req = httparse::Request::new(&mut headers);
        Ok(match try!(req.parse(buf)) {
            httparse::Status::Complete(len) => {
                trace!("Request.parse Complete({})", len);
                Some((MessageHead {
                    version: if req.version.unwrap() == 1 { Http11 } else { Http10 },
                    subject: (
                        try!(req.method.unwrap().parse()),
                        try!(req.path.unwrap().parse())
                    ),
                    headers: try!(Headers::from_raw(req.headers))
                }, len))
            },
            httparse::Status::Partial => None
        })
    }

    fn decoder(head: &MessageHead<Self::Incoming>) -> ::Result<Decoder> {
        use ::method::Method;
        use ::header;
        if head.subject.0 == Method::Get || head.subject.0 == Method::Head {
            Ok(Decoder::Length(0))
        } else if let Some(&header::ContentLength(len)) = head.headers.get() {
            Ok(Decoder::Length(len))
        } else if head.headers.has::<header::TransferEncoding>() {
            todo!("check for Transfer-Encoding: chunked");
            Ok(Decoder::Chunked(None))
        } else {
            Ok(Decoder::Length(0))
        }
    }


    fn encode<W: io::Write>(mut head: MessageHead<Self::Outgoing>, dst: &mut W) -> Encoder {
        use ::header;
        //debug!("writing head: {:?}", head);
        //let init_cap = 30 + headers.len() * AVERAGE_HEADER_SIZE +
        //    chunk.as_ref().map(|c| c.as_ref().len()).unwrap_or(0);
        //let mut buf = Vec::with_capacity(init_cap);

        if !head.headers.has::<header::Date>() {
            head.headers.set(header::Date(header::HttpDate(::time::now_utc())));
        }

        let mut is_chunked = true;
        let mut body = Encoder::chunked();
        if let Some(cl) = head.headers.get::<header::ContentLength>() {
            body = Encoder::length(**cl);
            is_chunked = false
        }

        if is_chunked {
            let encodings = match head.headers.get_mut::<header::TransferEncoding>() {
                Some(&mut header::TransferEncoding(ref mut encodings)) => {
                    //TODO: check if chunked is already in encodings. use HashSet?
                    encodings.push(header::Encoding::Chunked);
                    false
                },
                None => true
            };

            if encodings {
                head.headers.set(header::TransferEncoding(vec![header::Encoding::Chunked]));
            }
        }


        debug!("{:#?}", head.headers);
        let _ = write!(dst, "{} {}\r\n{}\r\n", head.version, head.subject, head.headers);

        body
    }
}

impl Http1Message for ClientMessage {
    type Incoming = RawStatus;
    type Outgoing = (Method, RequestUri);


    fn initial_interest() -> Next {
        Next::write()
    }

    fn parse(buf: &[u8]) -> ParseResult<RawStatus> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        trace!("Response.parse([Header; {}], [u8; {}])", headers.len(), buf.len());
        let mut res = httparse::Response::new(&mut headers);
        Ok(match try!(res.parse(buf)) {
            httparse::Status::Complete(len) => {
                trace!("Response.try_parse Complete({})", len);
                let code = res.code.unwrap();
                let reason = match StatusCode::from_u16(code).canonical_reason() {
                    Some(reason) if reason == res.reason.unwrap() => Cow::Borrowed(reason),
                    _ => Cow::Owned(res.reason.unwrap().to_owned())
                };
                Some((MessageHead {
                    version: if res.version.unwrap() == 1 { Http11 } else { Http10 },
                    subject: RawStatus(code, reason),
                    headers: try!(Headers::from_raw(res.headers))
                }, len))
            },
            httparse::Status::Partial => None
        })
    }

    fn decoder(_head: &MessageHead<Self::Incoming>) -> ::Result<Decoder> {
        unimplemented!()
    }

    fn encode<W: io::Write>(_head: MessageHead<Self::Outgoing>, _dst: &mut W) -> Encoder {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use httparse;

    use super::{parse};

    #[test]
    fn test_parse_request() {
        let raw = b"GET /echo HTTP/1.1\r\nHost: hyper.rs\r\n\r\n";
        parse::<httparse::Request, _>(raw).unwrap();
    }

    #[test]
    fn test_parse_raw_status() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\n";
        let (res, _) = parse::<httparse::Response, _>(raw).unwrap().unwrap();
        assert_eq!(res.subject.1, "OK");

        let raw = b"HTTP/1.1 200 Howdy\r\n\r\n";
        let (res, _) = parse::<httparse::Response, _>(raw).unwrap().unwrap();
        assert_eq!(res.subject.1, "Howdy");
    }

    #[cfg(feature = "nightly")]
    use test::Bencher;

    #[cfg(feature = "nightly")]
    #[bench]
    fn bench_parse_incoming(b: &mut Bencher) {
        let raw = b"GET /echo HTTP/1.1\r\nHost: hyper.rs\r\n\r\n";
        b.iter(|| {
            parse::<httparse::Request, _>(raw).unwrap()
        });
    }

}
