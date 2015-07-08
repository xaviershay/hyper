use std::cmp;
use std::io;

use http::{AtomicWrite};
use http::write_buf::WriteBuf;

/// Encoders to handle different Transfer-Encodings.
#[derive(Debug, Clone)]
pub struct Encoder {
    kind: Kind,
    prefix: Option<WriteBuf<Vec<u8>>>
}

impl Encoder {
    pub fn chunked() -> Encoder {
        Encoder {
            kind: Kind::Chunked,
            prefix: None
        }
    }

    pub fn length(len: u64) -> Encoder {
        Encoder {
            kind: Kind::Length(len),
            prefix: None
        }
    }

    pub fn prefix(&mut self, prefix: WriteBuf<Vec<u8>>) {
        self.prefix = Some(prefix);
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Kind {
    /// An Encoder for when Transfer-Encoding includes `chunked`.
    Chunked,
    /// An Encoder for when Content-Length is set.
    ///
    /// Enforces that the body is not longer than the Content-Length header.
    Length(u64),
}

impl Encoder {
    pub fn is_eof(&self) -> bool {
        match self.kind {
            Kind::Length(0) => true,
            _ => false
        }
    }

    pub fn encode<W: AtomicWrite>(&mut self, w: &mut W, msg: &[u8]) -> io::Result<usize> {
        match self.kind {
            Kind::Chunked => {
                unimplemented!("Chunked.encode()");
                /*
                let chunk_size = msg.len();
                trace!("chunked write, size = {:?}", chunk_size);
                try!(write!(w, "{:X}\r\n", chunk_size));
                try!(w.write_all(msg));
                try!(w.write_all(b"\r\n"));
                Ok(msg.len())
                */
            },
            Kind::Length(ref mut remaining) => {
                let mut n = {
                    let max = cmp::min(*remaining as usize, msg.len());
                    let slice = &msg[..max];

                    let prefix = self.prefix.as_ref().map(|buf| &buf.bytes[buf.pos..]).unwrap_or(b"");

                    try!(w.write_atomic(&[prefix, slice]))
                };

                if let Some(mut buf) = self.prefix.take() {
                    if buf.bytes.len() - buf.pos > n {
                        buf.pos += n;
                        self.prefix = Some(buf);
                        return Err(io::Error::new(io::ErrorKind::WouldBlock, "would block"));
                    } else {
                        let nbuf = buf.bytes.len() - buf.pos;
                        n -= nbuf;
                    }
                }

                *remaining -= n as u64;
                Ok(n)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Encoder;
    #[test]
    fn test_write_chunked() {
        let mut dst = Vec::new();
        let mut encoder = Encoder::Chunked;

        encoder.encode(&mut dst, b"foo bar").unwrap();
        encoder.encode(&mut dst, b"baz quux herp").unwrap();
        encoder.encode(&mut dst, b"").unwrap();
        assert_eq!(&dst[..], &b"7\r\nfoo bar\r\nD\r\nbaz quux herp\r\n0\r\n\r\n"[..]);
    }

    #[test]
    fn test_write_sized() {
        let mut dst = Vec::new();
        let mut encoder = Encoder::Length(8);
        encoder.encode(&mut dst, b"foo bar").unwrap();
        assert_eq!(encoder.encode(&mut dst, b"baz").unwrap(), 1);

        assert_eq!(dst, b"foo barb");
    }
}
