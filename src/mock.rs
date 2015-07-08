use std::cmp;
use std::io::{self, Read, Write};

pub struct Async<T> {
    inner: T,
    bytes_until_block: usize,
}

impl<T> Async<T> {
    pub fn new(inner: T, bytes: usize) -> Async<T> {
        Async {
            inner: inner,
            bytes_until_block: bytes
        }
    }

    pub fn block_in(&mut self, bytes: usize) {
        self.bytes_until_block = bytes;
    }
}

impl<T: Read> Read for Async<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.bytes_until_block == 0 {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "mock block"))
        } else {
            let n = cmp::min(self.bytes_until_block, buf.len());
            let n = try!(self.inner.read(&mut buf[..n]));
            self.bytes_until_block -= n;
            Ok(n)
        }
    }
}

impl<T: Write> Write for Async<T> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.bytes_until_block == 0 {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "mock block"))
        } else {
            let n = cmp::min(self.bytes_until_block, data.len());
            let n = try!(self.inner.write(&data[..n]));
            self.bytes_until_block -= n;
            Ok(n)
        }
    }

    fn flush(&mut self) -> io::Result<usize> {
        self.inner.flush()
    }
}
