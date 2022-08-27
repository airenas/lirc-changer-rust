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
    env_logger::init();
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
    log::info!("Starting IR eChanger");

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

    log::info!("Connected to '{}', waiting for messages...", in_path);
    let stream = BufReader::new(socket);

    if Path::new(out_path).exists() {
        std::fs::remove_file(out_path).unwrap();
    }
    let listener = UnixListener::bind(out_path).unwrap();
    log::info!("Connected to '{}', waiting for clients...", out_path);

    let (tx, rx) = unbounded();
    let (ptx, prx) = unbounded();
    let (rtx, rrx) = unbounded();

    let (tcl, rcl) = unbounded();
    let mut signals = Signals::new(&[SIGINT, SIGHUP, SIGTERM, SIGQUIT]).unwrap();

    thread::spawn(move || {
        let sig = signals.forever().next();
        println!("Received signal {:?}", sig);
        tcl.send(0).unwrap(); // drop sender after return, so it closes receivers
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
    log::info!("drop pipe file");
    std::fs::remove_file(out_path).unwrap();
    log::info!("Bye!");
}

enum Msg {
    Init(u32, Sender<String>),
    Close(u32),
}

fn handle_client(mut stream: UnixStream, info: crossbeam_channel::Sender<Msg>, num: u32) {
    log::info!("connected {}", num);
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    info.send(Msg::Init(num, tx)).unwrap();
    for received in rx {
        log::info!("Got: {}", &received);
        stream
            .write_all((received.clone() + "\n").as_bytes())
            .unwrap();
        log::info!("Wrote: {}", &received);
    }
    log::info!("disconnected {}", num);
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
        log::info!("Elapsed: {}.{:03} sec", dur.as_secs(), dur.subsec_millis());
    }

    let start = Instant::now();
    let mut update = tick(Duration::from_secs(1));

    loop {
        select! {
            recv(update) -> _ => {
                log::debug!("on timer");
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
                log::info!("Got process {}", received.to_str());
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
    log::info!("exit process");
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
            log::info!("Got {}", &s);
            let lr = rc.lock().unwrap();
            for (key, value) in lr.iter() {
                let s = s.clone();
                match value.send(s) {
                    Ok(_) => {
                        log::info!("send {}", key);
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
                        log::info!("Got init: {}", id);
                        let mut lr = l_receivers.lock().unwrap();
                        lr.insert(id, stream);
                        log::info!("Clients: {}", lr.len());
                    }
                    Msg::Close(id) => {
                        log::info!("Got close: {}", id);
                        let mut lr = l_receivers.lock().unwrap();
                        lr.remove(&id);
                        log::info!("Clients: {}", lr.len());
                    }
                }
            }
            recv(cl) -> _ => { break;}
        }
    }
    log::info!("exit broadcast");
}
