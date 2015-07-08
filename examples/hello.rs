#![deny(warnings)]
extern crate hyper;
extern crate env_logger;

use hyper::server::{Request, Response};

//static PHRASE: &'static [u8] = b"Hello World!";

fn hello(_: Request, _res: Response) {
    //res.send(PHRASE)
}

fn main() {
    env_logger::init().unwrap();
    let _listening = hyper::Server::http("127.0.0.1:3000").unwrap()
        .handle(hello).unwrap();
    println!("Listening on http://127.0.0.1:3000");
}
