use std::{io, net};
use std::str::FromStr;
use std::time::{Duration, Instant};

use actix::prelude::*;
use futures::StreamExt;
use tokio::io::{split, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::FramedRead;
