#![deny(warnings)]
extern crate hyper;
extern crate env_logger;

use std::io::Write;

use hyper::http::{Decoder, Encoder, Next};
use hyper::net::HttpStream;
use hyper::server::{Server, Handler, Request, Response};

static PHRASE: &'static [u8] = b"Hello World!";

struct Hello;

impl Handler<HttpStream> for Hello {
    fn on_request(&mut self, _: Request) -> Next {
        Next::write()
    }
    fn on_request_readable(&mut self, _: &mut Decoder<HttpStream>) -> Next {
        Next::write()
    }
    fn on_response(&mut self, response: &mut Response) -> Next {
        use hyper::header::ContentLength;
        response.headers_mut().set(ContentLength(PHRASE.len() as u64));
        Next::write()
    }
    fn on_response_writable(&mut self, encoder: &mut Encoder<HttpStream>) -> Next {
        let n = encoder.write(PHRASE).unwrap();
        debug_assert_eq!(n, PHRASE.len());
        Next::end()
    }
}

fn main() {
    env_logger::init().unwrap();
    let _listening = Server::http("127.0.0.1:3000").unwrap()
        .handle(|| Hello).unwrap();
    println!("Listening on http://127.0.0.1:3000");
}
