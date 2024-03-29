use std::{io, net, thread};
use std::io::Error;
use std::str::FromStr;
use std::time::Duration;

use actix::prelude::*;
use tokio::io::{split, WriteHalf};
use tokio::net::TcpStream;
use tokio_util::codec::FramedRead;

use crate::client::codec::ChatResponse;

mod codec;


#[actix_web::main]
async fn main() {
    // Connect to server
    let addr = net::SocketAddr::from_str("127.0.0.1:12345").unwrap();

    println!("Running chat client!");

    let stream = TcpStream::connect(&addr).await.unwrap();

    let addr = ChatClient::create(|ctx| {
        let (r, w) = split(stream);
        ChatClient::add_stream(FramedRead::new(r, codec::ClientChatCodec), ctx);
        ChatClient {
            framed: actix::io::FramedWrite::new(w, codec::ClientChatCodec, ctx),
        }
    });

    // start console loop
    thread::spawn(move || loop {
        let mut cmd = String::new();
        if io::stdin().read_line(&mut cmd).is_err() {
            println!("error");
            return;
        }
        addr.do_send(ClientCommand(cmd));
    });
}

struct ChatClient {
    framed: actix::io::FramedWrite<
        codec::ChatRequest,
        WriteHalf<TcpStream>,
        codec::ClientChatCodec,
    >,
}

#[derive(Message)]
#[rtype(result = "()")]
struct ClientCommand(String);

impl Actor for ChatClient {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // start heartbeats otherwise server will disconnect after 10 seconds
        self.hb(ctx)
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        println!("Disconnected");
        // stop application on discoonect
        System::current().stop();
    }
}

impl ChatClient {
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(1, 0), |act, ctx| {
            act.framed.write(codec::ChatRequest::Ping);
            act.hb(ctx);

            // client should also check for a timeout here, similar to the server code
        });
    }
}

impl actix::io::WriteHandler<io::Error> for ChatClient {}

/// Handle stdin commands
impl Handler<ClientCommand> for ChatClient {
    type Result = ();

    fn handle(&mut self, msg: ClientCommand, ctx: &mut Self::Context) -> Self::Result {
        let m = msg.0.trim();
        if m.is_empty() {
            return;
        }
        // we check for /sss type of messages
        if m.starts_with('/') {
            let v: Vec<&str> = m.splitn(2, ' ').collect();
            match v[0] {
                "/list" => {
                    self.framed.write(codec::ChatRequest::List);
                }
                "/join" => {
                    if v.len() == 2 {
                        self.framed.write(codec::ChatRequest::Join(v[1].to_owned()));
                    } else {
                        println!("!!! room name is required");
                    }
                }
                _ => println!("!!! unkown command"),
            }
        } else {
            self.framed.write(codec::ChatRequest::Message(m.to_owned()));
        }
    }
}


// server communication
impl StreamHandler<Result<codec::ChatResponse, io::Error>> for ChatClient {
    fn handle(&mut self, msg: Result<ChatResponse, Error>, ctx: &mut Self::Context) {
        match msg {
            Ok(codec::ChatResponse::Message(ref msg)) => {
                println!("message: {}", msg);
            }
            Ok(codec::ChatResponse::Joined(ref msg)) => {
                println!("!!! joined: {}", msg);
            }
            Ok(codec::ChatResponse::Rooms(rooms)) => {
                println!("\n!!! Available rooms.");
                for room in rooms {
                    println!("{}", room);
                }
                println!();
            }
            _ => ctx.stop(),
        }
    }
}