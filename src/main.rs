use lazy_static::lazy_static;
use std::{
    io::{self, ErrorKind, Read, Write},
    isize,
    os::fd::AsRawFd,
    process,
    sync::{Arc, RwLock, RwLockReadGuard},
};
use termios::*;

mod error;
pub mod prelude;

#[derive(Default)]
struct EditorConfigData {
    cursor_x: usize,
    cursor_y: usize,
    editor_num_rows: usize,
    editor_num_columns: usize,
    rows: Vec<String>,
    original_terminal: Option<Termios>,
}

impl EditorConfigData {
    pub fn append_row(&mut self, row: String) {
        self.rows.push(row);
    }

    // SETTERs
    pub fn set_original_terminal(&mut self, original_terminal: Termios) {
        self.original_terminal = Some(original_terminal);
    }

    pub fn set_dimensions(&mut self, num_rows: usize, num_columns: usize) {
        self.editor_num_rows = num_rows;
        self.editor_num_columns = num_columns;
    }

    pub fn set_cursor_x(&mut self, cursor_x: usize) {
        self.cursor_x = cursor_x;
    }

    pub fn set_cursor_y(&mut self, cursor_y: usize) {
        self.cursor_y = cursor_y;
    }

    // GETTERs
    pub fn get_editor_num_rows(&self) -> usize {
        self.editor_num_rows
    }

    pub fn get_editor_num_columns(&self) -> usize {
        self.editor_num_columns
    }

    pub fn get_original_terminal(&self) -> Option<Termios> {
        self.original_terminal
    }

    pub fn get_cursor(&self) -> (usize, usize) {
        (self.cursor_x, self.cursor_y)
    }

    pub fn get_text_num_rows(&self) -> usize {
        self.rows.len()
    }
}

/*** EditorConfig ***/
struct EditorConfig {
    data: RwLock<EditorConfigData>,
}

impl EditorConfig {
    pub fn append_row(&self, row: String) {
        self.data.write().unwrap().append_row(row)
    }

    // SETTERs
    pub fn set_original_terminal(&self, original_terminal: Termios) {
        self.data
            .write()
            .unwrap()
            .set_original_terminal(original_terminal);
    }

    pub fn set_dimensions(&self, num_rows: usize, num_columns: usize) {
        self.data
            .write()
            .unwrap()
            .set_dimensions(num_rows, num_columns)
    }

    pub fn set_cursor_x(&self, cursor_x: usize) {
        self.data.write().unwrap().set_cursor_x(cursor_x)
    }

    pub fn set_cursor_y(&self, cursor_y: usize) {
        self.data.write().unwrap().set_cursor_y(cursor_y)
    }

    pub fn set_rows(&self, rows: Vec<String>) {
        self.data.write().unwrap().rows = rows
    }

    // GETTERs
    pub fn get_editor_num_rows(&self) -> usize {
        self.data.read().unwrap().get_editor_num_rows()
    }

    pub fn get_editor_num_columns(&self) -> usize {
        self.data.read().unwrap().get_editor_num_columns()
    }

    pub fn get_original_terminal(&self) -> Option<Termios> {
        self.data.read().unwrap().get_original_terminal()
    }

    pub fn get_cursor(&self) -> (usize, usize) {
        self.data.read().unwrap().get_cursor()
    }

    pub fn acquire_read_lock(&self) -> RwLockReadGuard<EditorConfigData> {
        self.data.read().unwrap()
    }

    pub fn get_text_num_rows(&self) -> usize {
        self.data.read().unwrap().get_text_num_rows()
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

/*** Data ***/

/*** Constants ***/
const KILO_VERSION: &str = "0.0.1";

// Editor Keys
const ARROW_LEFT_KEY: usize = 1000;
const ARROW_RIGHT_KEY: usize = 1001;
const ARROW_UP_KEY: usize = 1002;
const ARROW_DOWN_KEY: usize = 1003;
const PAGE_UP_KEY: usize = 1004;
const PAGE_DOWN_KEY: usize = 1005;
const HOME_KEY: usize = 1006;
const END_KEY: usize = 1007;
const DELETE_KEY: usize = 1008;

/*** Static Variables ***/
lazy_static! {
    static ref EDITOR: Arc<EditorConfig> = Arc::new(EditorConfig {
        data: RwLock::new(EditorConfigData {
            cursor_x: 0,
            cursor_y: 0,
            editor_num_rows: 0,
            editor_num_columns: 0,
            rows: vec![],
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

fn ctrl_char(k: char) -> usize {
    ((k as u8) & 0x1f) as usize
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
    let editor = EDITOR.acquire_read_lock();

    let editor_num_rows = editor.get_editor_num_rows();
    let editor_num_columns = editor.get_editor_num_columns();

    for y in 0..editor_num_rows {
        if y >= editor.get_text_num_rows() {
            if editor.get_text_num_rows() == 0 && y == editor_num_rows / 3 {
                let mut welcome_msg = format!("Kilo editor -- version {}", KILO_VERSION);
                if welcome_msg.len() > editor_num_columns {
                    welcome_msg = welcome_msg[..editor_num_columns].to_string();
                }
                let mut padding = (editor_num_columns - welcome_msg.len()) / 2;
                if padding > 0 {
                    buffer.push("~");
                    padding -= 1;
                }

                while padding > 0 {
                    buffer.push(" ");
                    padding -= 1;
                }

                buffer.push(&welcome_msg);
            } else {
                buffer.push("~");
            }
        } else {
            let mut row: &str = &editor.rows[y];
            let row_len = row.len();
            if row_len > editor_num_columns {
                row = &row[..editor_num_columns];
            }
            buffer.push(row);
        }

        buffer.push("\x1b[K");
        if y < editor_num_rows - 1 {
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

/*** File I/O ***/
fn editor_open(filename: &str) {
    let file_contents = match std::fs::read_to_string(filename) {
        Ok(file) => file,
        Err(_) => {
            die("fopen");
            return;
        }
    };

    for line in file_contents.split('\n') {
        let mut length = line.len();
        while length > 0
            && (line.as_bytes()[length - 1] == b'\n' || line.as_bytes()[length - 1] == b'\r')
        {
            length -= 1;
        }

        let line = &line[..length];
        // println!("Append row: {} (len: {})", line, length);
        EDITOR.append_row(line.to_string());
    }
}

/*** Input ***/
// TODO: Refactor reading into buffer
fn editor_read_key() -> usize {
    let mut buf: [u8; 1] = [0; 1];

    if let Err(error) = io::stdin().lock().read_exact(&mut buf) {
        if error.kind() != ErrorKind::UnexpectedEof {
            die(&format!("Read error: {}", error));
        }
    }

    // Read escape squences
    if buf[0] as char == '\x1b' {
        let mut seq: [u8; 3] = [0; 3];

        // Read the next two characters (if no response assume escape key)
        if let Err(error) = io::stdin().lock().read_exact(&mut seq[..1]) {
            if error.kind() == ErrorKind::UnexpectedEof {
                return '\x1b' as usize; // Escape key
            }
            die(&format!("Read error: {}", error));
        }
        if let Err(error) = io::stdin().lock().read_exact(&mut seq[1..2]) {
            if error.kind() == ErrorKind::UnexpectedEof {
                return '\x1b' as usize; // Escape key
            }
            die(&format!("Read error: {}", error));
        }

        if seq[0] as char == '[' {
            if seq[1] as char > '0' && seq[1] as char <= '9' {
                if let Err(error) = io::stdin().lock().read_exact(&mut seq[2..3]) {
                    if error.kind() == ErrorKind::UnexpectedEof {
                        return '\x1b' as usize; // Escape key
                    }
                    die(&format!("Read error: {}", error));
                }

                if seq[2] as char == '~' {
                    match seq[1] as char {
                        '1' => return HOME_KEY,
                        '3' => return DELETE_KEY,
                        '4' => return END_KEY,
                        '5' => return PAGE_UP_KEY,
                        '6' => return PAGE_DOWN_KEY,
                        '7' => return HOME_KEY,
                        '8' => return END_KEY,
                        _ => {}
                    }
                }
            } else {
                match seq[1] as char {
                    'A' => return ARROW_UP_KEY,
                    'B' => return ARROW_DOWN_KEY,
                    'C' => return ARROW_RIGHT_KEY,
                    'D' => return ARROW_LEFT_KEY,
                    'H' => return HOME_KEY,
                    'F' => return END_KEY,
                    _ => {}
                }
            }
        } else if seq[0] as char == 'O' {
            match seq[1] as char {
                'H' => return HOME_KEY,
                'F' => return END_KEY,
                _ => {}
            }
        }

        return '\x1b' as usize;
    }

    buf[0] as usize
}

fn editor_move_cursor(key: usize) {
    fn move_cursor(offset_x: isize, offset_y: isize) {
        let (cursor_x, cursor_y) = EDITOR.get_cursor();
        let cursor_x = cursor_x as isize + offset_x;
        let cursor_y = cursor_y as isize + offset_y;

        let num_rows = EDITOR.get_editor_num_rows();
        let num_columns = EDITOR.get_editor_num_columns();

        if cursor_x >= 0 && cursor_x < num_columns as isize {
            EDITOR.set_cursor_x(cursor_x as usize);
        }

        if cursor_y >= 0 && cursor_y < num_rows as isize {
            EDITOR.set_cursor_y(cursor_y as usize);
        }
    }

    match key {
        ARROW_LEFT_KEY => move_cursor(-1, 0),
        ARROW_RIGHT_KEY => move_cursor(1, 0),
        ARROW_UP_KEY => move_cursor(0, -1),
        ARROW_DOWN_KEY => move_cursor(0, 1),
        _ => {}
    }
}

/** Returns true if should continue */
fn editor_process_keypress() {
    let key: usize = editor_read_key();

    // Exit on q
    match key {
        _ if key == ctrl_char('q') => {
            cleanup();
            process::exit(0);
        }
        ARROW_LEFT_KEY | ARROW_RIGHT_KEY | ARROW_UP_KEY | ARROW_DOWN_KEY => editor_move_cursor(key),
        PAGE_DOWN_KEY | PAGE_UP_KEY => {
            let mut times = EDITOR.get_editor_num_rows() as isize;
            while times > 0 {
                editor_move_cursor(if key == PAGE_UP_KEY {
                    ARROW_UP_KEY
                } else {
                    ARROW_DOWN_KEY
                });
                times -= 1;
            }
        }
        HOME_KEY => EDITOR.set_cursor_x(0),
        END_KEY => {
            let num_columns = EDITOR.get_editor_num_columns();
            EDITOR.set_cursor_x(num_columns - 1);
        }
        _ => {}
    };
}

fn main() {
    enable_raw_mode();
    init_editor();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        editor_open(&args[1]);
    }

    loop {
        editor_refresh_screen();
        editor_process_keypress();
    }
}
