fn main() {}
/*
#![deny(warnings)]
extern crate hyper;

extern crate env_logger;

use std::env;
use std::io::{self, Write};

use hyper::Client;
use hyper::header::Connection;

fn main() {
    env_logger::init().unwrap();

    let url = match env::args().nth(1) {
        Some(url) => url,
        None => {
            println!("Usage: client <url>");
            return;
        }
    };

    let client = match Client::new() {
        Ok(c) => c,
        Err(e) => {
            println!("Error creating Client: {}", e);
            return;
        }
    };
    client.get(&url)
        .header(Connection::close())
        .send(|res| {
            println!("Response: {}", res.status());
            println!("Headers:\n{}", res.headers());
            res.stream(|bytes| {
                match bytes {
                    Ok(Some(bytes)) => io::stdout().write_all(bytes).unwrap(),
                    Ok(None) => println!("\n\n\tdone."),
                    Err(e) => println!("\n\n\tError reading response: {}", e)
                }
            })
        });
}
*/
