use std::{
    borrow::Borrow,
    io::{self, ErrorKind, Read, Write},
    os::fd::AsRawFd,
    process,
    sync::Mutex,
};

use termios::*;

mod error;
pub mod prelude;

static ORIGINAL_TERMINAL: Mutex<Option<Termios>> = Mutex::new(None);

// lazy_static! {
//     static ref my_mutex: Mutex<i32> = Mutex::new(0i32);
// }

fn enable_raw_mode() {
    let original_termios = Termios::from_fd(io::stdin().as_raw_fd()).unwrap();

    let mut termios = original_termios;

    termios.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    termios.c_oflag &= !(OPOST);
    termios.c_cflag |= CS8;
    termios.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    termios.c_cc[VMIN] = 0;
    termios.c_cc[VTIME] = 1;

    // Apply the updated termios settings
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, &termios).unwrap();

    *(ORIGINAL_TERMINAL.lock().unwrap()) = Some(original_termios);
}

fn disable_terminal(original_termios: &Termios) {
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, original_termios).unwrap();
}

fn cleanup() {
    write_to_stdout("\x1b[2J");
    write_to_stdout("\x1b[H");

    flush_stdout();

    if let Some(original_termios) = ORIGINAL_TERMINAL.lock().unwrap().as_ref() {
        disable_terminal(original_termios);
    }
}

fn die(s: &str) {
    cleanup();
    eprintln!("Error: {}", s);
    process::exit(1);
}

fn ctrl_char(k: char) -> u8 {
    (k as u8) & 0x1f
}

fn write_to_stdout(s: &str) {
    let mut stdout = io::stdout().lock();
    let write_ok = stdout.write(s.as_bytes());
    if let Err(error) = write_ok {
        die(&format!("Write error: {}", error));
    }
}

fn flush_stdout() {
    let mut stdout = io::stdout().lock();
    let flush_ok = stdout.flush();
    if let Err(error) = flush_ok {
        die(&format!("Flush error: {}", error));
    }
}

/** Requires a flush to be guaranteed on the screen */
fn editor_draw_rows() {
    for y in 0..24 {
        write_to_stdout("~\r\n");
    }
}

fn editor_refresh_screen() {
    // Clear screen
    write_to_stdout("\x1b[2J");

    // Position at the top of the screen
    write_to_stdout("\x1b[H");

    editor_draw_rows();

    write_to_stdout("\x1b[H");

    flush_stdout();
}

fn editor_read_key() -> u8 {
    let mut buf: [u8; 1] = [0; 1];

    let read_ok: Result<(), io::Error> = io::stdin().lock().read_exact(&mut buf);
    if let Err(error) = read_ok {
        if error.kind() != ErrorKind::UnexpectedEof {
            die(&format!("Read error: {}", error));
        }
    }

    buf[0]
}

/** Returns true if should continue */
fn editor_process_keypress() {
    let c: u8 = editor_read_key();

    // Exit on q
    match c {
        _ if c == ctrl_char('q') => {
            cleanup();
            process::exit(0);
        }
        _ => {}
    };
}

fn main() {
    enable_raw_mode();

    loop {
        editor_refresh_screen();
        editor_process_keypress();
    }
}
