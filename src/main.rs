use std::{
    io::{self, ErrorKind, Read, Write},
    os::fd::AsRawFd,
    process,
};

use termios::*;

mod error;
pub mod prelude;

struct Cleanup {
    original_termios: Option<Termios>,
}

impl Cleanup {
    pub fn default() -> Cleanup {
        Cleanup {
            original_termios: None,
        }
    }

    pub fn set_original_termios(&mut self, original_terminal: Termios) {
        self.original_termios = Some(original_terminal);
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        println!("Running cleanup");
        if let Some(original_termios) = &self.original_termios {
            disable_terminal(original_termios);
        }
    }
}

fn enable_raw_mode(cleanup: &mut Cleanup) {
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

    cleanup.set_original_termios(original_termios);
}

fn disable_terminal(original_terminal: &Termios) {
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, original_terminal).unwrap();
}

fn die(s: &str) {
    eprintln!("Error: {}", s);
    process::exit(1);
}

fn ctrl_char(k: char) -> u8 {
    (k as u8) & 0x1f
}

fn write_to_stdout(s : &str) {
    let mut stdout = io::stdout().lock();
    let write_ok = stdout.write(s.as_bytes());
    if let Err(error) = write_ok {
        die(&format!("Write error: {}", error));
    }

    let flush_ok = stdout.flush();
    if let Err(error) = flush_ok {
        die(&format!("Flush error: {}", error));
    }
}

fn editor_refresh_screen() {
    // Clear screen
    write_to_stdout("\x1b[2J");

    // Position at the top of the screen
    write_to_stdout("\x1b[H");
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
fn editor_process_keypress() -> bool {
    let c: u8 = editor_read_key();

    // Exit on q
    match c {
        _ if c == ctrl_char('q') => {
            return false;
        }
        _ => {}
    };

    true
}

fn main() {
    let mut cleanup = Cleanup::default();

    enable_raw_mode(&mut cleanup);

    loop {
        editor_refresh_screen();
        let result = editor_process_keypress();
        if !result {
            break;
        }
    }
}
