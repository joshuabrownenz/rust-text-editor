use std::{
    io::{self, ErrorKind, Read},
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

fn ctrl_char(k : char) -> u8 {
    (k as u8) & 0x1f
}

fn main() {
    let mut cleanup = Cleanup::default();

    enable_raw_mode(&mut cleanup);

    loop {
        let mut buf: [u8; 1] = [0; 1];
        let read_ok: Result<(), io::Error> = io::stdin().lock().read_exact(&mut buf);
        if let Err(error) = read_ok {
            if error.kind() != ErrorKind::UnexpectedEof {
                die(&format!("Read error: {}", error));
            }
        }

        let char: u8 = buf[0];
        let is_control_char: bool = unsafe { libc::iscntrl(char as i32) == 1 };

        if is_control_char {
            print!("{}\r\n", char);
        } else {
            print!("{} ('{}')\r\n", char, char as char);
        }

        // Exit on q
        if char == ctrl_char('q') {
            break;
        }
    }
}
