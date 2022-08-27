mod event;

use clap::{App, Arg};
use crossbeam_channel::never;
use crossbeam_channel::{select, tick, unbounded};
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM},
    iterator::Signals,
};
use std::collections::HashMap;
use std::io::prelude::*;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::Instant;

fn main() {
    let matches = App::new("changer")
        .version("0.1")
        .author("Airenas V.<airenass@gmail.com>")
        .about("Listens for IR events and calculates HOLD events")
        .arg(
            Arg::new("socketIn")
                .short('i')
                .long("input")
                .value_name("FILE")
                .help("Sets a socket in path")
                .takes_value(true),
        )
        .arg(
            Arg::new("socketOut")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Sets a socket output path")
                .takes_value(true),
        )
        .get_matches();
    println!("Starting IR eChanger");

    let in_path = matches
        .value_of("socketIn")
        .unwrap_or("/var/run/lirc/lircd");
    let out_path = matches
        .value_of("socketOut")
        .unwrap_or("/var/run/lirc/lircd2");
    let socket = match UnixStream::connect(in_path) {
        Ok(sock) => sock,
        Err(e) => {
            eprintln!("Couldn't connect to {}: {:?}", in_path, e);
            return;
        }
    };

    println!("Connected to '{}', waiting for messages...", in_path);
    let stream = BufReader::new(socket);

    if Path::new(out_path).exists() {
        std::fs::remove_file(out_path).unwrap();
    }
    let listener = UnixListener::bind(out_path).unwrap();
    println!("Connected to '{}', waiting for clients...", out_path);

    let (tx, rx) = unbounded();
    let (ptx, prx) = unbounded();
    let (rtx, rrx) = unbounded();

    let (tcl, rcl) = unbounded();
    let mut signals = Signals::new(&[SIGINT, SIGHUP, SIGTERM, SIGQUIT]).unwrap();

    thread::spawn(move || {
        for sig in signals.forever() {
            println!("Received signal {:?}", sig);
            tcl.send(0).unwrap(); // drop sender after return, so it closes receivers
            return;
        }
    });

    thread::spawn(move || {
        stream
            .lines()
            .map(|l| l.unwrap())
            .map(|l| {
                println!("{}", l);
                l
            })
            .map(|l| event::Event::from_str(&l))
            .filter_map(|r| r.map_err(|e| eprintln!("{}", e)).ok())
            .for_each(|l| tx.send(l).unwrap())
    });

    let rtct = rcl.clone();
    thread::spawn(move || process(rx, ptx, rtct));

    let rtxc = rtx.clone();
    let tj = thread::spawn(move || broadcast(prx, rrx, rtxc, rcl));

    let mut num = 0;

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let rtxc = rtx.clone();
                    num += 1;
                    thread::spawn(move || handle_client(stream, rtxc, num));
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    break;
                }
            }
        }
    });

    tj.join().unwrap();
    println!("drop pipe file");
    std::fs::remove_file(out_path).unwrap();
    println!("Bye!");
}

enum Msg {
    Init(u32, Sender<String>),
    Close(u32),
}

fn handle_client(mut stream: UnixStream, info: crossbeam_channel::Sender<Msg>, num: u32) {
    println!("connected {}", num);
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    info.send(Msg::Init(num, tx)).unwrap();
    for received in rx {
        println!("Got: {}", &received);
        stream
            .write_all((received.clone() + "\n").as_bytes())
            .unwrap();
        println!("Wrote: {}", &received);
    }
    println!("disconnected {}", num);
    info.send(Msg::Close(num)).unwrap();
}

fn process(
    data: crossbeam_channel::Receiver<event::Event>,
    out: crossbeam_channel::Sender<event::Event>,
    cl: crossbeam_channel::Receiver<u32>,
) {
    let mut prev: Option<event::Event> = None;
    let mut at = Instant::now();
    fn show(dur: Duration) {
        println!("Elapsed: {}.{:03} sec", dur.as_secs(), dur.subsec_millis());
    }

    let start = Instant::now();
    let mut update = tick(Duration::from_secs(1));

    loop {
        select! {
            recv(update) -> _ => {
                println! {"on timer"}
                show(start.elapsed());
                match prev {
                    None => {
                    }
                    Some(e) => {
                        let now = Instant::now();
                        if now > at + Duration::from_millis(500) {
                            out.send(e.to_hold()).unwrap();
                        } else{
                            out.send(e.to_new()).unwrap();
                        }
                        prev = None;
                    }
                }
                update = never()
            }
            recv(data) -> msg => {
                show(start.elapsed());
                let received = msg.unwrap();
                println! {"Got process {}", received.to_str()}
                let now = Instant::now();
                match prev {
                    None => {
                        println! {"none"}
                        if received.repeat == 0 {
                            prev = Some(received);
                            at = now;
                        }
                    }
                    Some(e) => {
                        prev = None;
                        if e.name != received.name {
                            println! {"!=name"}
                            out.send(e).unwrap();
                            prev = Some(received);
                            at = now;
                        } else if e.repeat + 1 != received.repeat {
                            println! {"!=repeat"}
                            out.send(e).unwrap();
                            if received.repeat == 0 {
                                prev = Some(received);
                                at = now;
                            }
                        } else if now > at + Duration::from_millis(500) {
                            println! {"long"}
                            out.send(e.to_hold()).unwrap();
                        } else{
                            println! {"skip"}
                            prev = Some(received)
                        }
                    }
                }
                update = tick(Duration::from_millis(100));
            }
            recv(cl) -> _ => { break;}
        }
    }
    println!("exit process");
}

fn broadcast(
    data: crossbeam_channel::Receiver<event::Event>,
    info: crossbeam_channel::Receiver<Msg>,
    close_info: crossbeam_channel::Sender<Msg>,
    cl: crossbeam_channel::Receiver<u32>,
) {
    let receivers: HashMap<u32, Sender<String>> = HashMap::new();
    let l_receivers = Arc::new(Mutex::new(receivers));
    let rc = l_receivers.clone();
    thread::spawn(move || {
        for received in data {
            let s = received.to_str();
            println! {"Got {}", &s}
            let lr = rc.lock().unwrap();
            for (key, value) in lr.iter() {
                let s = s.clone();
                match value.send(s) {
                    Ok(_) => {
                        println!("send {}", key);
                    }
                    Err(err) => {
                        eprintln!("Can't send to {}. {}", key, err);
                        close_info.send(Msg::Close(*key)).unwrap();
                    }
                }
            }
        }
    });

    loop {
        select! {
            recv(info) -> msg => {
                let received = msg.unwrap();
                match received {
                    Msg::Init(id, stream) => {
                        println!("Got init: {}", id);
                        let mut lr = l_receivers.lock().unwrap();
                        lr.insert(id, stream);
                        println!("Clients: {}", lr.len());
                    }
                    Msg::Close(id) => {
                        println!("Got close: {}", id);
                        let mut lr = l_receivers.lock().unwrap();
                        lr.remove(&id);
                        println!("Clients: {}", lr.len());
                    }
                }
            }
            recv(cl) -> _ => { break;}
        }
    }
    println!("exit broadcast");
}
