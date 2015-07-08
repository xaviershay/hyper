#![deny(warnings)]
extern crate hyper;
extern crate env_logger;

extern crate eventual;

use hyper::{Get, Post, Streaming};
use hyper::header::ContentLength;
use hyper::server::{Server, Request, Response};
use hyper::uri::RequestUri::AbsolutePath;


fn handle(req: Request, mut res: Response) {
    match *req.uri() {
        AbsolutePath(ref path) => match (req.method(), &path[..]) {
            (&Get, "/") | (&Get, "/echo") => {
                res.send(b"Try POST /echo");
                return;
            },
            (&Post, "/echo") => (), // fall through, fighting mutable borrows
            _ => {
                *res.status_mut() = hyper::NotFound;
                return;
            }
        },
        _ => {
            return;
        }
    };

    if let Some(len) = req.headers().get::<ContentLength>() {
        res.headers_mut().set(*len);
    }
    res.start(move |result| {
        match result {
            Ok(res) => echo(req, res),
            Err(e) => println!("error writing response head: {:?}", e)
        }
    });
}

fn echo(req: Request, res: Response<Streaming>) {
    req.read(move |result| {
        match result {
            Ok((data, req)) => {
                res.write_all(data, move |result| {
                    match result {
                        Ok(res) => echo(req, res),
                        Err(e) => println!("write error: {:?}", e)
                    }
                });
            }
            Err(e) => {
                println!("read error: {:?}", e);
            }
        }
    })
}

fn main() {
    env_logger::init().unwrap();
    let server = Server::http("127.0.0.1:1337").unwrap();
    let _guard = server.handle(handle);
    println!("Listening on http://127.0.0.1:1337");
}
