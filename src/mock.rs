use std::cmp;
use std::io::{self, Read, Write};

pub struct Async {
    data: Vec<u8>,
    bytes_until_block: usize,
}

impl Async {
    pub fn new(data: Vec<u8>, bytes_until_block: usize) -> Async {
        Async {
            data: data,
            bytes_until_block: bytes_until_block
        }
    }

    pub fn block_in(&mut self, bytes: usize) {
        self.bytes_until_block = bytes;
    }
}

impl Read for Async {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        unimplemented!()
        /*
        if self.bytes_until_block == 0 {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "wouldblock"))
        } else {
            let i = cmp::min(self.bytes_until_block, buf.len());
            self.bytes_until_block -= i;
            self.data.read(&mut buf[..i])
        }
        */
    }
}

impl Write for Async {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.bytes_until_block == 0 {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "wouldblock"))
        } else {
            let i = cmp::min(self.bytes_until_block, data.len());
            self.bytes_until_block -= i;
            self.data.write(&data[..i])
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
