use std::io::{self, Write};

use self::Encoder::{Chunked, Length, Through};

/// Encoders to handle different Transfer-Encodings.
#[derive(Debug)]
pub enum Encoder {
    /// A pass-through Encoder, used initially before Transfer-Encoding is determined.
    Through,
    /// A Encoder for when Transfer-Encoding includes `chunked`.
    Chunked,
    /// A Encoder for when Content-Length is set.
    ///
    /// Enforces that the body is not longer than the Content-Length header.
    Length(u64),
}

impl Encoder {
    pub fn is_eof(&self) -> bool {
        match *self {
            Length(0) => true,
            _ => false
        }
    }

    pub fn encode<W: Write>(&mut self, w: &mut W, msg: &[u8]) -> io::Result<usize> {
        match *self {
            Through => w.write(msg),
            Chunked => {
                let chunk_size = msg.len();
                trace!("chunked write, size = {:?}", chunk_size);
                try!(write!(w, "{:X}\r\n", chunk_size));
                try!(w.write_all(msg));
                try!(w.write_all(b"\r\n"));
                Ok(msg.len())
            },
            Length(ref mut remaining) => {
                let len = msg.len() as u64;
                if len > *remaining {
                    let len = *remaining;
                    *remaining = 0;
                    try!(w.write_all(&msg[..len as usize]));
                    Ok(len as usize)
                } else {
                    *remaining -= len;
                    try!(w.write_all(msg));
                    Ok(len as usize)
                }
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
