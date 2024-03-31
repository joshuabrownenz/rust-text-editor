use std::{
    io::{self, Read},
    os::fd::{AsFd, AsRawFd},
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
            original_termios: None
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

fn main() {
    let mut cleanup = Cleanup::default();

    enable_raw_mode(&mut cleanup);

    let mut buf: [u8; 1] = [0; 1];
    loop {
        let read = io::stdin().lock().read_exact(&mut buf);
        if read.is_err() {
            println!("Read error: {}", read.err().unwrap());
            break;
        }

        // Exit on q
        if buf == "q".as_bytes() {
            break;
        }

        println!("Recv: {:?}", &buf)
    }
}

fn enable_raw_mode(cleanup: &mut Cleanup) {
    let original_termios = Termios::from_fd(io::stdin().as_raw_fd()).unwrap();

    let mut termios = original_termios;

    // Turn off ECHO
    termios.c_lflag &= !(ECHO | ICANON);

    // Apply the updated termios settings
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, &termios).unwrap();

    cleanup.set_original_termios(original_termios);
}

fn disable_terminal(original_terminal: &Termios) {
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, original_terminal).unwrap();
}
