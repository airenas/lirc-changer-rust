use clap::{App, Arg};
use std::io::prelude::*;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

fn main() {
    env_logger::init();
    let matches = App::new("listener")
        .version("0.1")
        .author("Airenas V.<airenass@gmail.com>")
        .about("Listens for socket events and prints it to stdout")
        .arg(
            Arg::new("socketIn")
                .short('i')
                .long("input")
                .value_name("FILE")
                .help("Sets a socket in path")
                .takes_value(true),
        )
        .get_matches();
    log::info!("Starting listener");

    let in_path = matches.value_of("socketIn").unwrap_or("test");
    let in_path_s: String = in_path.into();

    thread::spawn(move || {
        let mut fail_count = 0;
        loop {
            log::info!("Try connect");
            let socket = match UnixStream::connect(in_path_s.clone()) {
                Ok(sock) => sock,
                Err(e) => {
                    fail_count += 1;
                    log::error!(
                        "Couldn't connect to {}, fail={}: {:?}",
                        in_path_s, fail_count, e
                    );
                    let mut wait_time = 500 + u64::pow(fail_count, 2) * 100;
                    if wait_time > 5000 {
                        wait_time = 5000;
                    }
                    log::info!("Waiting {} ms", wait_time);
                    thread::sleep(Duration::from_millis(wait_time));
                    continue;
                }
            };
            fail_count = 0;
            log::info!("Connected to '{}', waiting for messages...", in_path_s);
            let stream = BufReader::new(socket);

            stream
                .lines()
                .map(|l| {
                    log::info!("Read");
                    l.unwrap()
                })
                .map(String::from)
                .map(|l| {
                    log::info!("GOT: {}", l);
                    l
                })
                .count();
                log::info!("Exit socket stream");
        }
    });

    let (tx, rx) = channel();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");
    rx.recv().expect("Could not receive from channel.");

    log::info!("Bye!");
}
