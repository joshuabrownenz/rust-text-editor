use std::{
    io::{self, Read},
    os::fd::{AsFd, AsRawFd},
};

use termios::*;

mod error;
pub mod prelude;

fn main() {
    enable_raw_mode();

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

fn enable_raw_mode() {
    let mut termios = Termios::from_fd(io::stdin().as_raw_fd()).unwrap();

    // Turn off ECHO
    termios.c_lflag &= !(ECHO);

    // Apply the updated termios settings
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, &termios).unwrap();
}
