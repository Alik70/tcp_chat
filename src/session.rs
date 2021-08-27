use std::{io, net};
use std::io::Error;
use std::str::FromStr;
use std::time::{Duration, Instant};

use actix::prelude::*;
use futures::StreamExt;
use tokio::io::{split, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::FramedRead;

use crate::codec::{ChatCodec, ChatRequest, ChatResponse};
use crate::server::{self, ChatServer};

// chat server sends this message to session
#[derive(Message)]
#[rtype(result = "()")]
pub struct Message(pub String);

/// ChatSession actor is responsible for tcp peer communication
pub struct ChatSession {
    /// unique session id
    id: usize,
    /// this is address of chat server
    addr: Addr<ChatServer>,
    /// Client mus send ping at least onece per 10 seconds, otherwise we drop connection.
    hb: Instant,
    /// joined room
    room: String,
    /// framed wrapper
    framed: actix::io::FramedWrite<ChatResponse, WriteHalf<TcpStream>, ChatCodec>,
}

impl Actor for ChatSession {
    /// For tcp communitcation we are going to use FramedContext.
    /// It is convenient wrapper around Framed object from tokio_io
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // we'll start the heartbeat process on session start
        self.hb(ctx);

        // register self in chat server AsyncContext::wait register
        // future within context, but context waits untill this future resolves
        // before processing any other events.
        let addr = ctx.address();
        self.addr
            .send(server::Connect {
                addr: addr.recipient(),
            })
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(res) => act.id = res,
                    // sth is wrong with chat server
                    _ => ctx.stop(),
                }
                actix::fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        // notify chat server
        self.addr.do_send(server::Disconnect { id: self.id });
        Running::Stop
    }
}

impl actix::io::WriteHandler<io::Error> for ChatSession {}

/// to use framed we have to define io type and codec
impl StreamHandler<Result<ChatRequest, io::Error>> for ChatSession {
    /// this is main event loop for client requests
    fn handle(&mut self, msg: Result<ChatRequest, Error>, ctx: &mut Self::Context) {
        match msg {
            Ok(ChatRequest::List) => {
                // send listrooms message to chat server and wait for response
                println!("List rooms");
                self.addr
                    .send(server::ListRooms)
                    .into_actor(self)
                    .then(|res, act, _| {
                        match res {
                            Ok(rooms) => {
                                act.framed.write(ChatResponse::Rooms(rooms));
                            }
                            _ => println!("Something went wrong"),
                        }
                        actix::fut::ready(())
                    })
                    .wait(ctx)
                // .wait(ctx) pauses all events in context, so actor wont receive any new messages until it get list of rooms back
            }
            Ok(ChatRequest::Join(name)) => {
                println!("Join to room: {}", name);
                self.room = name.clone();
                self.addr.do_send(server::Join {
                    id: self.id,
                    name: name.clone(),
                });
                self.framed.write(ChatResponse::Joined(name));
            }
            Ok(ChatRequest::Message(message)) => {
                // send message to chat server
                println!("Peer message: {}", message);
                self.addr.do_send(server::Message {
                    id: self.id,
                    msg: message,
                    room: self.room.clone(),
                })
            }
            // we update heartbeat time on ping from peer
            Ok(ChatRequest::Ping) => self.hb = Instant::now(),
            _ => ctx.stop(),
        }
    }
}

/// Handler for Message, chat server sends this message, we just send string to peer
impl Handler<Message> for ChatSession {
    type Result = ();
    fn handle(&mut self, msg: Message, _: &mut Self::Context) -> Self::Result {
        // send message to peer
        self.framed.write(ChatResponse::Message(msg.0));
    }
}

/// Helper methods
impl ChatSession {
    pub fn new(
        addr: Addr<ChatServer>,
        framed: actix::io::FramedWrite<ChatResponse, WriteHalf<TcpStream>, ChatCodec>,
    ) -> ChatSession {
        ChatSession {
            id: 0,
            addr,
            hb: Instant::now(),
            room: "Main".to_owned(),
            framed,
        }
    }
    /// helper method that sends ping to client every second.
    /// also this method check heartbeats from client
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_interval(Duration::new(1, 0), |act, ctx| {
            // check client heatbeats
            if Instant::now().duration_since(act.hb) > Duration::new(10, 0) {
                // heatbeat timed out
                println!("Client heatbeat failed, disconnecting!");

                // notify chat server
                act.addr.do_send(server::Disconnect { id: act.id });

                // stop actor
                ctx.stop();
            }
            act.framed.write(ChatResponse::Ping);
            // if we can not send message to sink, sink is closed (disconnected)
        });
    }
}

/// Define tcp server that will accept incoming tcp connection and create chat actors.
pub fn tcp_server(_s: &str, server: Addr<ChatServer>) {
    // Create serve listener
    let addr = net::SocketAddr::from_str("127.0.0.1:12345").unwrap();

    actix_web::rt::spawn(async move {
        let server = server.clone();
        let mut listener = TcpListener::bind(&addr).await.unwrap();
        let mut incoming = listener.incoming();


        while let Some(stream) = incoming.next().await {
            match stream {
                Ok(stream) => {
                    let server = server.clone();
                    ChatSession::create(|ctx| {
                        let (r, w) = split(stream);
                        ChatSession::add_stream(FramedRead::new(r, ChatCodec), ctx);
                        ChatSession::new(
                            server,
                            actix::io::FramedWrite::new(w, ChatCodec, ctx),
                        )
                    });
                }
                Err(_) => return,
            }
        }
    });
}