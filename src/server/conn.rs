use std::sync::Arc;

use httparse;

use http;
use net;

use super::{Handler, request, response};

pub struct Conn<H: Handler> {
    handler: Arc<H>
}

impl<H: Handler> Conn<H> {
    pub fn new(handler: Arc<H>) -> Conn<H> {
        Conn {
            handler: handler,
        }
    }
}

impl<H: Handler> http::Handler for Conn<H> {
    type Incoming = httparse::Request<'static, 'static>;
    type Outgoing = http::Response;

    fn on_incoming(&mut self, head: http::RequestHead,
                   inc: http::IncomingStream,
                   out: http::OutgoingStream<http::Response, net::Fresh>) {
        let request = request::new(head, inc);
        let response = response::new(out);
        self.handler.handle(request, response);
    }
}
