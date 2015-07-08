use std::io;

pub trait Read {
    fn on_read(&mut self, body: &mut io::Read) -> io::Result<()>;
}

pub trait Write {
    fn on_write(&mut self, body: &mut io::Write) -> io::Result<()>;
}

pub trait Data {
    fn on_data(&mut self, data: &[u8]);
    fn on_error(&mut self, err: ::Error);
    fn on_eof(&mut self);
}

impl<F> Read for F where F: FnMut(&mut io::Read) -> io::Result<()> {
    fn on_read(&mut self, body: &mut io::Read) -> io::Result<()> {
        self(body)
    }
}

impl<F> Data for F where F: FnMut(::Result<Option<&[u8]>>) {
    fn on_data(&mut self, data: &[u8]) {
        self(Ok(Some(data)))
    }

    fn on_error(&mut self, err: ::Error) {
        self(Err(err))
    }

    fn on_eof(&mut self) {
        self(Ok(None))
    }
}

pub struct ReadOnce<F: FnOnce(::Result<&[u8]>) + Send + 'static> {
    callback: Option<F>
}

impl<F> ReadOnce<F> where F: FnOnce(::Result<&[u8]>) + Send + 'static {
    pub fn new(callback: F) -> ReadOnce<F> {
        ReadOnce { callback: Some(callback) }
    }
}

impl<F> Read for ReadOnce<F> where F: FnOnce(::Result<&[u8]>) + Send + 'static {
    fn on_read(&mut self, transport: &mut io::Read) -> io::Result<()> {
        debug_assert!(self.callback.is_some());
        let mut buf = [0u8; 4096];
        match transport.read(&mut buf) {
            Ok(n) => {
                self.callback.take().map(move |cb| cb(Ok(&buf[..n])));
                Ok(())
            },
            Err(e) => {
                let kind = e.kind();
                if kind == io::ErrorKind::WouldBlock {
                    Err(e)
                } else {
                    let cause = e.to_string();
                    self.callback.take().map(move |cb| cb(Err(e.into())));
                    Err(io::Error::new(kind, cause))
                }
            }
        }
    }
}

pub struct WriteAll<T: AsRef<[u8]>, F: FnOnce(::Result<()>) + Send + 'static> {
    buf: T,
    pos: usize,
    callback: Option<F>
}

impl<T, F> WriteAll<T, F> where T: AsRef<[u8]>, F: FnOnce(::Result<()>) + Send + 'static {
    pub fn new(buf: T, callback: F) -> WriteAll<T, F> {
        trace!("WriteAll::new({} bytes)", buf.as_ref().len());
        WriteAll {
            buf: buf,
            pos: 0,
            callback: Some(callback)
        }
    }
}

impl<T, F> Write for WriteAll<T, F> where T: AsRef<[u8]>, F: FnOnce(::Result<()>) + Send + 'static {
    fn on_write(&mut self, body: &mut io::Write) -> io::Result<()> {
        trace!("write_all {}..{}", self.pos, self.buf.as_ref().len());
        debug_assert!(self.callback.is_some());
        while self.pos < self.buf.as_ref().len() {
            match body.write(&self.buf.as_ref()[self.pos..]) {
                Ok(n) => self.pos += n,
                Err(e) => match e.kind() {
                    io::ErrorKind::Interrupted => (),
                    io::ErrorKind::WouldBlock => return Err(e),
                    _ => {
                        trace!("write_all error: {:?}", e);
                        let copy = io::Error::new(e.kind(), format!("{}", e));
                        self.callback.take().map(move |callback| callback(Err(e.into())));
                        return Err(copy);
                    }
                }
            }
        }
        trace!("wrote all {}", self.buf.as_ref().len());
        self.callback.take().map(|callback| callback(Ok(())));
        Ok(())
    }
}
