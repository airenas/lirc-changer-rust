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
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::Instant;

fn main() -> ExitCode {
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
            log::error!("Couldn't connect to {}: {:?}", in_path, e);
            return ExitCode::FAILURE;
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

    let (t_close, r_close) = unbounded();
    let (t_close_main, r_close_main): (
        crossbeam_channel::Sender<i32>,
        crossbeam_channel::Receiver<i32>,
    ) = unbounded();
    let t_exit = thread::spawn(move || {
        if let Some(sig) = r_close_main.into_iter().next() {
            log::debug!("Got main exit event {}", sig);
            drop(t_close);
            return sig;
        }    
        0
    });

    let mut signals = Signals::new(&[SIGINT, SIGHUP, SIGTERM, SIGQUIT]).unwrap();
    let tclm = t_close_main.clone();
    thread::spawn(move || {
        for (num, sig) in signals.forever().enumerate() {
            log::debug!("Received signal {:?}, {} time(s)", sig, num);
            tclm.send(sig).unwrap();
        }
    });

    thread::spawn(move || {
        stream
            .lines()
            .map(|l| l.unwrap())
            .map(|l| {
                log::info!("{}", l);
                l
            })
            .map(|l| event::Event::from_str(&l))
            .filter_map(|r| r.map_err(|e| log::error!("{}", e)).ok())
            .for_each(|l| tx.send(l).unwrap())
    });

    let mut threads = vec![];

    let r_close_cl = r_close.clone();
    threads.push(thread::spawn(move || {
        process(rx, ptx, r_close_cl);
        match t_close_main.send(2) {
            Ok(_) => {}
            Err(err) => {
                log::warn!("{}", err);
            }
        };
    }));

    let rtxc = rtx.clone();
    threads.push(thread::spawn(move || broadcast(prx, rrx, rtxc, r_close)));

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
                    log::error!("Error: {}", err);
                    break;
                }
            }
        }
    });

    threads.into_iter().for_each(|h| h.join().unwrap());
    let ec = t_exit.join().unwrap();

    log::info!("drop pipe file '{}'", out_path);
    std::fs::remove_file(out_path).unwrap();

    log::info!("Bye!");
    ExitCode::from(u8::try_from(ec).unwrap())
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
        match stream.write_all((received.clone() + "\n").as_bytes()) {
            Ok(_) => {
                log::info!("Wrote: {}", &received);
            }
            Err(err) => {
                log::warn!("Can't write to {}. {}", num, err);
                break;
            }
        }
    }
    log::info!("disconnected {}", num);

    match info.send(Msg::Close(num)) {
        Ok(_) => {}
        Err(err) => {
            log::warn!("{}", err);
        }
    }
}

fn process(
    data: crossbeam_channel::Receiver<event::Event>,
    out: crossbeam_channel::Sender<event::Event>,
    cl: crossbeam_channel::Receiver<u32>,
) {
    let mut prev: Option<event::Event> = None;
    let mut at = Instant::now();
    fn show(dur: Duration) {
        log::debug!("Elapsed: {}.{:03} sec", dur.as_secs(), dur.subsec_millis());
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
                let received = match msg {
                    Ok(msg) => msg,
                    Err(err) => {
                        log::warn!("{}", err);
                        break;
                    }
                };
                log::info!("Got process {}", received.to_str());
                let now = Instant::now();
                match prev {
                    None => {
                        log::debug!("none");
                        if received.repeat == 0 {
                            prev = Some(received);
                            at = now;
                        }
                    }
                    Some(e) => {
                        prev = None;
                        if e.name != received.name {
                            log::debug!("!=name");
                            out.send(e).unwrap();
                            prev = Some(received);
                            at = now;
                        } else if e.repeat + 1 != received.repeat {
                            log::debug!("!=repeat");
                            out.send(e).unwrap();
                            if received.repeat == 0 {
                                prev = Some(received);
                                at = now;
                            }
                        } else if now > at + Duration::from_millis(500) {
                            log::debug!("long");
                            out.send(e.to_hold()).unwrap();
                        } else{
                            log::debug!("skip");
                            prev = Some(received)
                        }
                    }
                }
                update = tick(Duration::from_millis(100));
            }
            recv(cl) -> _ => {
                log::debug!("event from close channel in process");
                break;
            }
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
                        log::error!("Can't send to {}. {}", key, err);
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
            recv(cl) -> _ => {
                log::debug!("event from close channel in broadcast");
                break;
            }
        }
    }
    log::info!("exit broadcast");
}
