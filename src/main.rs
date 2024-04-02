use lazy_static::lazy_static;
use std::{
    io::{self, ErrorKind, Read, Write},
    os::fd::AsRawFd,
    process,
    sync::{Arc, RwLock},
};
use termios::*;

mod error;
pub mod prelude;

#[derive(Default)]
struct EditorData {
    cursor_x: usize,
    cursor_y: usize,
    num_rows: usize,
    num_columns: usize,
    original_terminal: Option<Termios>,
}

/*** EditorConfig ***/
struct EditorConfig {
    data: RwLock<EditorData>,
}

impl EditorConfig {
    // SETTERs
    pub fn set_original_terminal(&self, original_terminal: Termios) {
        let mut data = self.data.write().unwrap();
        data.original_terminal = Some(original_terminal);
    }

    pub fn set_dimensions(&self, num_rows: usize, num_columns: usize) {
        let mut data = self.data.write().unwrap();
        data.num_rows = num_rows;
        data.num_columns = num_columns;
    }

    pub fn set_cursor_x(&self, cursor_x: usize) {
        let mut data = self.data.write().unwrap();
        data.cursor_x = cursor_x;
    }

    pub fn set_cursor_y(&self, cursor_y: usize) {
        let mut data = self.data.write().unwrap();
        data.cursor_y = cursor_y;
    }

    // GETTERs
    pub fn get_num_rows(&self) -> usize {
        self.data.read().unwrap().num_rows
    }

    pub fn get_num_columns(&self) -> usize {
        self.data.read().unwrap().num_columns
    }

    pub fn get_original_terminal(&self) -> Option<Termios> {
        self.data.read().unwrap().original_terminal
    }

    pub fn get_cursor(&self) -> (usize, usize) {
        let data = self.data.read().unwrap();
        (data.cursor_x, data.cursor_y)
    }
}

/*** AppendBuffer ***/
struct AppendBuffer {
    buf: String,
}

impl AppendBuffer {
    pub fn new() -> Self {
        AppendBuffer { buf: String::new() }
    }

    pub fn push(&mut self, s: &str) {
        self.buf.push_str(s)
    }

    pub fn write(self) {
        write_to_stdout(&self.buf);
        flush_stdout();
    }
}

/*** Constants ***/
const KILO_VERSION: &str = "0.0.1";

/*** Static Variables ***/
lazy_static! {
    static ref EDITOR: Arc<EditorConfig> = Arc::new(EditorConfig {
        data: RwLock::new(EditorData {
            cursor_x: 0,
            cursor_y: 0,
            num_rows: 0,
            num_columns: 0,
            original_terminal: None,
        }),
    });
}

/*** Terminal ***/
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

    EDITOR.set_original_terminal(original_termios);
}

fn disable_terminal(original_termios: &Termios) {
    tcsetattr(io::stdin().as_raw_fd(), TCSANOW, original_termios).unwrap();
}

fn cleanup() {
    write_to_stdout("\x1b[2J");
    write_to_stdout("\x1b[H");

    flush_stdout();

    if let Some(original_termios) = &EDITOR.get_original_terminal() {
        disable_terminal(original_termios);
    }
}

fn die(s: &str) {
    cleanup();
    eprintln!("Error: {}", s);
    process::exit(1);
}

fn get_window_size() {
    if let Some((w, h)) = term_size::dimensions() {
        EDITOR.set_dimensions(h, w);
    } else {
        die("get dimensions");
    }
}

fn ctrl_char(k: char) -> char {
    ((k as u8) & 0x1f) as char
}

/*** Output ***/
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

fn init_editor() {
    EDITOR.set_cursor_x(0);
    EDITOR.set_cursor_y(0);
    get_window_size();
}

/*** Editor ***/

/** Requires a flush to be guaranteed on the screen */
fn editor_draw_rows(buffer: &mut AppendBuffer) {
    let num_rows = EDITOR.get_num_rows();
    let num_columns = EDITOR.get_num_columns();
    for y in 0..num_rows {
        if y == num_rows / 3 {
            let mut welcome_msg = format!("Kilo editor -- version {}", KILO_VERSION);
            if welcome_msg.len() > num_columns {
                welcome_msg = welcome_msg[..num_columns].to_string();
            }
            let mut padding = (num_columns - welcome_msg.len()) / 2;
            if padding > 0 {
                buffer.push("~");
                padding -= 1;
            }

            while (padding > 0) {
                buffer.push(" ");
                padding -= 1;
            }

            buffer.push(&welcome_msg);
        } else {
            buffer.push("~");
        }

        buffer.push("\x1b[K");
        if y < num_rows - 1 {
            buffer.push("\r\n");
        }
    }
}

fn editor_refresh_screen() {
    let mut buffer = AppendBuffer::new();

    // Hide cursor
    buffer.push("\x1b[?25l");

    // Position at the top of the screen
    buffer.push("\x1b[H");

    editor_draw_rows(&mut buffer);

    // Position cursor at cursor_x and cursor_y
    let (cursor_x, cursor_y) = EDITOR.get_cursor();
    buffer.push(&format!("\x1b[{};{}H", cursor_y + 1, cursor_x + 1));

    // Show cursor
    buffer.push("\x1b[?25h");

    buffer.write();
}

/*** Input ***/
fn editor_read_key() -> char {
    let mut buf: [u8; 1] = [0; 1];

    let read_ok: Result<(), io::Error> = io::stdin().lock().read_exact(&mut buf);
    if let Err(error) = read_ok {
        if error.kind() != ErrorKind::UnexpectedEof {
            die(&format!("Read error: {}", error));
        }
    }

    buf[0] as char
}

fn editor_move_cursor(key: u8) {
    fn move_cursor(offset_x: isize, offset_y: isize) {
        let (cursor_x, cursor_y) = EDITOR.get_cursor();
        let cursor_x = cursor_x as isize + offset_x;
        let cursor_y = cursor_y as isize + offset_y;

        let num_rows = EDITOR.get_num_rows();
        let num_columns = EDITOR.get_num_columns();

        if cursor_x >= 0 && cursor_x < num_columns as isize {
            EDITOR.set_cursor_x(cursor_x as usize);
        }
        
        if cursor_y >= 0 && cursor_y < num_rows as isize {
            EDITOR.set_cursor_y(cursor_y as usize);
        }
    }

    match key as char {
        'h' => move_cursor(-1, 0),
        'l' => move_cursor(1, 0),
        'k' => move_cursor(0, -1),
        'j' => move_cursor(0, 1),
        _ => {}
    }
}

/** Returns true if should continue */
fn editor_process_keypress() {
    let c: char = editor_read_key();

    // Exit on q
    match c {
        _ if c == ctrl_char('q') => {
            cleanup();
            process::exit(0);
        }
        'h' | 'j' | 'k' | 'l' => editor_move_cursor(c as u8),
        _ => {}
    };
}

fn main() {
    enable_raw_mode();
    init_editor();

    loop {
        editor_refresh_screen();
        editor_process_keypress();
    }
}
