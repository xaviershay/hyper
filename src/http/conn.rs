use std::io;
use std::marker::PhantomData;
use std::mem;

use tick::{self, Protocol, Transport, Interest};

use http::{self, h1, MessageHead, Parse};
use http::buffer::Buffer;
use net::Fresh;

const MAX_BUFFER_SIZE: usize = 8192 + 4096 * 100;

pub struct Conn<T: tick::Transport, H: Handler> {
    state: State,
    transfer: tick::Transfer,
    buf: Buffer,
    handler: H,
    _marker: PhantomData<T>
}

impl<T: Transport, H: Handler> Conn<T, H> {
    pub fn new(transfer: tick::Transfer, handler: H) -> Conn<T, H> {
        Conn {
            state: State::Parsing,
            transfer: transfer,
            buf: Buffer::new(),
            handler: handler,
            _marker: PhantomData,
        }
    }

    fn interest(&mut self) -> tick::Interest {
        let mut state = mem::replace(&mut self.state, State::Closed);
        let i = match state {
            State::Parsing => tick::Interest::Read,
            State::Http1 { ref mut incoming, ref mut outgoing }  => {
                match (incoming.next(), outgoing.next()) {
                    (Next::Eof, Next::Eof) => {
                        if incoming.keep_alive() && outgoing.keep_alive() {
                            debug!("keep-alive loop continue");
                            self.state = State::Parsing;
                            return self.interest();
                        } else {
                            return self.interest();
                        }
                    }
                    (Next::Continue, Next::Continue) => Interest::ReadWrite,
                    (Next::Continue, _) => Interest::Read,
                    (_, Next::Continue) => Interest::Write,
                    _ => Interest::Wait,
                }
            }
            //State::Http2
            State::Closed => tick::Interest::Remove
        };
        self.state = state;
        trace!("interest: {:?}", i);
        i
    }
}

impl<T: Transport, H: Handler> Protocol<T> for Conn<T, H> {
    fn on_readable(&mut self, transport: &mut T) -> io::Result<tick::Interest> {
        let action = match self.state {
            State::Parsing => {
                try!(self.buf.read_from(transport));
                match http::parse::<H::Incoming, _>(self.buf.bytes()) {
                    Ok(Some((head, len))) => {
                        trace!("parsed {} bytes out of {}", len, self.buf.len());
                        self.buf.consume(len);
                        //let h1_conn = h1::Conn::new(head, &self.transfer, &self.handler);
                        match H::Incoming::decoder(&head) {
                            Ok(decoder) => {
                                let keep_alive = head.should_keep_alive();
                                let (inc_tx, inc_rx) = h1::incoming(decoder, self.transfer.clone(), keep_alive);
                                let (out_tx, out_rx) = h1::outgoing(self.transfer.clone(), keep_alive);

                                self.handler.on_incoming(
                                    head,
                                    inc_tx,
                                    out_tx
                                );
                                let new_state = State::Http1 {
                                    incoming: inc_rx,
                                    outgoing: out_rx,
                                };
                                if self.buf.is_empty() {
                                    Action::State(new_state)
                                } else {
                                    Action::OnData(new_state)
                                }
                            },
                            Err(e) => {
                                debug!("error creating decoder: {:?}", e);
                                //TODO: respond with 400
                                Action::State(State::Closed)
                            }
                        }
                    },
                    Ok(None) => {
                        if self.buf.len() >= MAX_BUFFER_SIZE {
                            //TODO: Handler.on_too_large_error()
                            debug!("MAX_BUFFER_SIZE reached, closing");
                            //self.transfer.abort();
                            Action::State(State::Closed)
                        } else {
                            Action::Nothing
                        }
                    },
                    Err(e) => {
                        trace!("parsing error: {:?}", e);
                        let h2_init = b"PRI * HTTP/2";
                        if self.buf.bytes().starts_with(h2_init) {
                            trace!("HTTP/2 request!");
                            //TODO: self.state = State::Http2(h2::conn());
                            //self.transfer.abort();
                            Action::State(State::Closed)
                        } else {
                            //TODO: match on error to send proper response
                            //TODO: have Handler.on_parse_error() or something
                            //self.transfer.close();
                            Action::State(State::Closed)
                        }
                    }
                }
            },
            State::Http1 { ref mut incoming, .. } => { //(ref mut conn) => {
                //conn.on_read(data);
                if !self.buf.is_empty() {
                    try!(incoming.on_read(&mut self.buf.wrap(transport)))
                } else {
                    try!(incoming.on_read(transport));
                }
                Action::Nothing
            },
            /*
            State::Http2(ref mut conn) => {
                conn.on_read(data);
            }
            */
            State::Closed => {
                error!("on_readable State::Closed");
                Action::Nothing
            }

        };

        match action {
            Action::State(state) => {
                self.state = state;
                Ok(self.interest())
            }
            Action::OnData(state) => {
                self.state = state;
                self.on_readable(transport)
            }
            Action::Nothing => Ok(self.interest()),
        }
    }

    fn on_writable(&mut self, transport: &mut T) -> io::Result<tick::Interest> {
        match self.state {
            State::Parsing(..) => trace!("on_writable State::Parsing"),
            State::Http1 { ref mut outgoing, .. } => {
                try!(outgoing.on_write(transport));
            },
            //State::Http2
            State::Closed => error!("on_writable State::Closed"),
        }

        Ok(self.interest())
    }

    fn on_error(&mut self, error: ::tick::Error) {
        error!("on_error {:?}", error);
        self.state = State::Closed;
    }

    fn on_remove(self, _transport: T) {
        trace!("on_remove, dropping transport");
    }
}


enum State {
    Parsing,
    Http1 {
        incoming: h1::Incoming,
        outgoing: h1::Outgoing,
    }, //(h1::Conn),
    //Http2,
    Closed,
}

enum Action {
    State(State),
    OnData(State),
    Nothing,
}

pub enum Next {
    Continue,
    Pause,
    Eof,
}

pub trait Handler {
    type Incoming: Parse;
    type Outgoing;
    fn on_incoming(&mut self,
                   head: MessageHead<<Self::Incoming as Parse>::Subject>,
                   incoming: http::IncomingStream,
                   outgoing: http::OutgoingStream<Self::Outgoing, Fresh>);
}


