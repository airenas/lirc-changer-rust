use clap::{App, Arg};
use std::collections::HashMap;
use std::io::prelude::*;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::thread::JoinHandle;
use std::time;

fn main() {
    env_logger::init();
    let matches = App::new("sender")
        .version("0.1")
        .author("Airenas V.<airenass@gmail.com>")
        .about("Sends unix socket events from stdout")
        .arg(
            Arg::new("socketOut")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Sets a socket output path")
                .takes_value(true),
        )
        .get_matches();
    log::info!("Starting sender");

    let out_path = matches.value_of("socketOut").unwrap_or("test");
    if Path::new(out_path).exists() {
        std::fs::remove_file(out_path).unwrap();
    }
    log::info!("Opening incomming socket {}", out_path);
    let listener = match UnixListener::bind(out_path) {
        Err(_) => panic!("failed to bind socket"),
        Ok(stream) => stream,
    };

    let (tj, rx) = spawn_stdin_channel();
    let (t1, r1) = mpsc::channel();
    thread::spawn(move || map(rx, t1));

    let receivers: HashMap<u32, Sender<String>> = HashMap::new();
    let l_receivers = Arc::new(Mutex::new(receivers));
    let rc = l_receivers.clone();
    thread::spawn(move || broadcast(r1, rc));

    thread::spawn(move || {
        let mut num = 0;
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    num += 1;
                    let cr = l_receivers.clone();
                    thread::spawn(move || handle_client(stream, cr, num));
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    break;
                }
            }
        }
    });

    log::info!("Waiting for stdin close");
    tj.join().unwrap();

    log::info!("Removing pipe");
    std::fs::remove_file(out_path).unwrap();
    log::info!("Bye!");
}

fn handle_client(
    mut stream: UnixStream,
    receivers: Arc<Mutex<HashMap<u32, Sender<String>>>>,
    num: u32,
) {
    let (txl, rxl) = mpsc::channel::<String>();
    {
        let mut lr = receivers.lock().unwrap();
        lr.insert(num, txl);
        log::info!("connected {}. len = {}", num, lr.len());
    }
    stream
        .write_all(format!("Hi {}\n", num).as_bytes())
        .unwrap();
    for received in rxl {
        match stream.write_all(received.as_bytes()) {
            Ok(_) => {
                log::info!("Wrote msg to {}", num);
            }
            Err(err) => {
                log::error!("err: {}", err);
                break;
            }
        }
    }
    let mut lr = receivers.lock().unwrap();
    lr.remove(&num);
    log::info!("disconnected {}. len = {}", num, lr.len());
}

enum Input {
    String(String),
    Close(),
}

fn spawn_stdin_channel() -> (JoinHandle<()>, mpsc::Receiver<String>) {
    let (tx, rx) = mpsc::channel::<Input>();
    let txc = tx.clone();
    thread::spawn(move || {
        log::info!("Start stdin thread");
        loop {
            let mut buffer = String::new();
            std::io::stdin().read_line(&mut buffer).unwrap();
            {
                txc.send(Input::String(buffer)).unwrap();
            }
        }
    });
    ctrlc::set_handler(move || {
        log::info!("Stop stdin");
        tx.send(Input::Close()).unwrap();
    })
    .expect("Error setting Ctrl-C handler");
    let (res_tx, res_rx) = mpsc::channel::<String>();
    let res = thread::spawn(move || {
        log::info!("Start input thread");
        for received in rx {
            match received {
                Input::String(string) => {
                    log::info!("Got str: {}", string);
                    let s = string.trim();
                    if !s.is_empty() {
                        res_tx.send(s.to_string()).unwrap();
                    }
                }
                Input::Close() => {
                    log::info!("Got close event");
                    drop(res_tx);
                    break;
                }
            }
        }
        log::info!("Stop input thread");
    });
    (res, res_rx)
}

fn broadcast(data: mpsc::Receiver<String>, receivers: Arc<Mutex<HashMap<u32, Sender<String>>>>) {
    for received in data {
        log::info! {"Got from stdin {}", received}
        let tmp = received + "\n";
        let lr = receivers.lock().unwrap();
        for (key, value) in lr.iter() {
            let s = tmp.clone();
            match value.send(s) {
                Ok(_) => {
                    log::info!("send {} to {}", tmp, key);
                }
                Err(err) => {
                    log::error!("Can't send to {}: {}", key, err);
                }
            }
        }
    }
    log::info!("Stopped broadcast");
}

fn map(data: mpsc::Receiver<String>, out: Sender<String>) {
    for received in data {
        log::info! {"Got from stdin {}", received}
        if received == "s" {
            out.send(String::from("qwe 0 KEY_UP device")).unwrap()
        } else if received == "a" {
            for i in 0..10 {
                if i > 0 {
                    thread::sleep(time::Duration::from_millis(80));
                }
                out.send(format!("qwe {} KEY_UP device", i)).unwrap();
            }
        } else {
            out.send(received).unwrap()
        }
    }
    drop(out);
    log::info!("Stopped map");
}
