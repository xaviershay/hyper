use std::cmp;
use std::io::{self, Write};

use http::internal::{AtomicWrite, WriteBuf};

/// Encoders to handle different Transfer-Encodings.
#[derive(Debug, Clone)]
pub struct Encoder {
    kind: Kind,
    prefix: Prefix, //Option<WriteBuf<Vec<u8>>>
}

impl Encoder {
    pub fn chunked() -> Encoder {
        Encoder {
            kind: Kind::Chunked(Chunked::Init),
            prefix: Prefix(None)
        }
    }

    pub fn length(len: u64) -> Encoder {
        Encoder {
            kind: Kind::Length(len),
            prefix: Prefix(None)
        }
    }

    pub fn prefix(&mut self, prefix: WriteBuf<Vec<u8>>) {
        self.prefix.0 = Some(prefix);
    }
}

#[derive(Debug, PartialEq, Clone)]
enum Kind {
    /// An Encoder for when Transfer-Encoding includes `chunked`.
    Chunked(Chunked),
    /// An Encoder for when Content-Length is set.
    ///
    /// Enforces that the body is not longer than the Content-Length header.
    Length(u64),
}


impl Encoder {
    pub fn is_eof(&self) -> bool {
        if self.prefix.0.is_some() {
            return false;
        }
        match self.kind {
            Kind::Length(0) => true,
            _ => false
        }
    }

    pub fn encode<W: AtomicWrite>(&mut self, w: &mut W, msg: &[u8]) -> io::Result<usize> {
        match self.kind {
            Kind::Chunked(ref mut chunked) => {
                let mut size = ChunkSize {
                    bytes: [0; CHUNK_SIZE_MAX_BYTES],
                    pos: 0,
                    len: 0,
                };
                trace!("chunked write, size = {:?}", msg.len());
                write!(&mut size, "{:X}", msg.len())
                    .expect("CHUNK_SIZE_MAX_BYTES should fit any usize");

                let mut n = {
                    let prefix = self.prefix.0.as_ref().map(|buf| &buf.bytes[buf.pos..]).unwrap_or(b"");
                    let pieces = [
                        prefix,
                        &size.bytes[size.pos.into() .. size.len.into()],
                        &b"\r\n"[..],
                        msg,
                        &b"\r\n"[..],
                    ];
                    try!(w.write_atomic(&pieces))
                };

                n = self.prefix.update(n);
                if n == 0 {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "would block"));
                }

                n = size.update(n);
                if n == 0 {
                    *chunked = if size.len == 0 {
                        Chunked::SizeNewline(false)
                    } else {
                        Chunked::Size(size)
                    };
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "would block"));
                }

                if n == 1 {
                    *chunked = Chunked::SizeNewline(true);
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "would block"));
                }

                n -= 2; // chunk size newline

                unimplemented!("Encoder::chunked() <- {}", n);
                //Ok(n)
            },
            Kind::Length(ref mut remaining) => {
                let mut n = {
                    let max = cmp::min(*remaining as usize, msg.len());
                    let slice = &msg[..max];

                    let prefix = self.prefix.0.as_ref().map(|buf| &buf.bytes[buf.pos..]).unwrap_or(b"");

                    try!(w.write_atomic(&[prefix, slice]))
                };

                n = self.prefix.update(n);
                if n == 0 {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "would block"));
                }

                *remaining -= n as u64;
                Ok(n)
            },
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
enum Chunked {
    Init,
    Size(ChunkSize),
    SizeNewline(bool),
    /*Body(usize),
    Newline,*/
    //End,
}

#[cfg(target_pointer_width = "32")]
const USIZE_BYTES: usize = 32;

#[cfg(target_pointer_width = "64")]
const USIZE_BYTES: usize = 64;

// each byte will become 2 hex
const CHUNK_SIZE_MAX_BYTES: usize = USIZE_BYTES * 2;

struct ChunkSize {
    bytes: [u8; CHUNK_SIZE_MAX_BYTES],
    pos: u8,
    len: u8,
}

impl ChunkSize {
    fn update(&mut self, n: usize) -> usize {
        let diff = (self.len - self.pos).into();
        if n >= diff {
            self.pos = 0;
            self.len = 0;
            n - diff
        } else {
            self.pos += n as u8; // just verified it was a small usize
            0
        }
    }
}

impl ::std::fmt::Debug for ChunkSize {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        f.debug_struct("ChunkSize")
            .field("bytes", &&self.bytes[..self.len.into()])
            .field("pos", &self.pos)
            .finish()
    }
}

impl Clone for ChunkSize {
    fn clone(&self) -> ChunkSize {
        let mut bytes = [0; CHUNK_SIZE_MAX_BYTES];
        let _ = (&mut bytes[..]).write(&self.bytes[..]);
        ChunkSize {
            bytes: self.bytes,
            pos: self.pos,
            len: self.len,
        }
    }
}

impl ::std::cmp::PartialEq for ChunkSize {
    fn eq(&self, other: &ChunkSize) -> bool {
        self.len == other.len &&
            self.pos == other.pos &&
            (&self.bytes[..]) == (&other.bytes[..])
    }
}

impl io::Write for ChunkSize {
    fn write(&mut self, msg: &[u8]) -> io::Result<usize> {
        let n = (&mut self.bytes[self.len.into() ..]).write(msg)
            .expect("&mut [u8].write() cannot error");
        self.len += n as u8; // safe because bytes is never bigger than 256
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Prefix(Option<WriteBuf<Vec<u8>>>);

impl Prefix {
    fn update(&mut self, n: usize) -> usize {
        if let Some(mut buf) = self.0.take() {
            if buf.bytes.len() - buf.pos > n {
                buf.pos += n;
                self.0 = Some(buf);
                0
            } else {
                let nbuf = buf.bytes.len() - buf.pos;
                n - nbuf
            }
        } else {
            n
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Encoder;
    use mock::{Async, Buf};

    #[test]
    fn test_write_chunked_sync() {
        let mut dst = Buf::new();
        let mut encoder = Encoder::chunked();

        encoder.encode(&mut dst, b"foo bar").unwrap();
        encoder.encode(&mut dst, b"baz quux herp").unwrap();
        encoder.encode(&mut dst, b"").unwrap();
        assert_eq!(&dst[..], &b"7\r\nfoo bar\r\nD\r\nbaz quux herp\r\n0\r\n\r\n"[..]);
    }

    #[test]
    fn test_write_chunked_async() {
        use std::io;
        let mut dst = Async::new(Buf::new(), 7);
        let mut encoder = Encoder::chunked();

        assert_eq!(4, encoder.encode(&mut dst, b"foo bar").unwrap());
        dst.block_in(6);
        assert_eq!(3, encoder.encode(&mut dst, b"bar").unwrap());
        assert_eq!(io::ErrorKind::WouldBlock, encoder.encode(&mut dst, b"baz quux herp").unwrap_err().kind());
        //encoder.encode(&mut dst, b"").unwrap();
        assert_eq!(&dst[..], &b"7\r\nfoo bar\r\nD\r\nbaz quux herp\r\n0\r\n\r\n"[..]);
    }

    #[test]
    fn test_write_sized() {
        let mut dst = Buf::new();
        let mut encoder = Encoder::length(8);
        encoder.encode(&mut dst, b"foo bar").unwrap();
        assert_eq!(encoder.encode(&mut dst, b"baz").unwrap(), 1);

        assert_eq!(dst, b"foo barb");
    }
}
