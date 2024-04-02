use std::{
    io::{self, ErrorKind, Read, Write},
    isize,
    os::fd::AsRawFd,
    process,
};
use termios::*;

mod error;
pub mod prelude;

/*** Constants ***/
const KILO_VERSION: &str = "0.0.1";
const KILO_TAB_STOP: usize = 8;

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

struct EditorRow {
    chars: String,
    render: String,
}

impl EditorRow {
    pub fn new(chars: String) -> Self {
        EditorRow {
            chars,
            render: String::new(),
        }
    }

    pub fn update_render(&mut self) {
        // Render tabs
        let mut tabs = 0;
        for c in self.chars.chars() {
            if c == '\t' {
                tabs += 1;
            }
        }

        let mut render = String::with_capacity(self.chars.len() + tabs * (KILO_TAB_STOP - 1));
        for c in self.chars.chars() {
            if c == '\t' {
                render.push_str(&" ".repeat(KILO_TAB_STOP));
            } else {
                render.push(c);
            }
        }

        self.render = render;
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

    pub fn write(self, editor: &mut Editor) {
        editor.write_to_stdout(&self.buf);
        editor.flush_stdout();
    }
}

/*** Editor ***/
struct Editor {
    cursor_x: usize,
    cursor_y: usize,
    row_offset: usize,
    column_offset: usize,
    editor_num_rows: usize,
    editor_num_columns: usize,
    rows: Vec<EditorRow>,
    original_terminal: Option<Termios>,
}

impl Editor {
    pub fn new() -> Self {
        let mut editor = Self {
            cursor_x: 0,
            cursor_y: 0,
            row_offset: 0,
            column_offset: 0,
            editor_num_rows: 0,
            editor_num_columns: 0,
            rows: vec![],
            original_terminal: None,
        };

        editor.get_window_size();

        editor
    }

    pub fn append_row(&mut self, row: String) {
        let mut editor_row = EditorRow::new(row);
        editor_row.update_render();
        self.rows.push(editor_row);
    }

    pub fn set_original_terminal(&mut self, original_terminal: Termios) {
        self.original_terminal = Some(original_terminal);
    }

    pub fn set_dimensions(&mut self, num_rows: usize, num_columns: usize) {
        self.editor_num_rows = num_rows;
        self.editor_num_columns = num_columns;
    }

    pub fn get_text_num_rows(&self) -> usize {
        self.rows.len()
    }

    /*** Terminal ***/
    fn enable_raw_mode(&mut self) {
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

        self.set_original_terminal(original_termios);
    }

    fn disable_terminal(&self) {
        if let Some(original_termios) = &self.original_terminal {
            tcsetattr(io::stdin().as_raw_fd(), TCSANOW, original_termios).unwrap();
        }
    }

    fn cleanup(&self) {
        self.write_to_stdout("\x1b[2J");
        self.write_to_stdout("\x1b[H");

        self.flush_stdout();

        self.disable_terminal();
    }

    fn die(&self, s: &str) {
        self.cleanup();
        eprintln!("Error: {}", s);
        process::exit(1);
    }

    fn get_window_size(&mut self) {
        if let Some((w, h)) = term_size::dimensions() {
            self.set_dimensions(h, w);
        } else {
            self.die("get dimensions");
        }
    }

    fn ctrl_char(k: char) -> usize {
        ((k as u8) & 0x1f) as usize
    }

    /*** Output ***/
    fn write_to_stdout(&self, s: &str) {
        let mut stdout = io::stdout().lock();
        let write_ok = stdout.write(s.as_bytes());
        if let Err(error) = write_ok {
            self.die(&format!("Write error: {}", error));
        }
    }

    fn flush_stdout(&self) {
        let mut stdout = io::stdout().lock();
        let flush_ok = stdout.flush();
        if let Err(error) = flush_ok {
            self.die(&format!("Flush error: {}", error));
        }
    }

    /*** Editor ***/
    fn editor_scroll(&mut self) {
        // Row offset
        if self.cursor_y < self.row_offset {
            self.row_offset = self.cursor_y;
        }

        if self.cursor_y >= self.row_offset + self.editor_num_rows {
            self.row_offset = self.cursor_y - self.editor_num_rows + 1;
        }

        // Column offset
        if self.cursor_x < self.column_offset {
            self.column_offset = self.cursor_x;
        }

        if self.cursor_x >= self.column_offset + self.editor_num_columns {
            self.column_offset = self.cursor_x - self.editor_num_columns + 1;
        }
    }

    /** Requires a flush to be guaranteed on the screen */
    fn editor_draw_rows(&self, buffer: &mut AppendBuffer) {
        let editor_num_rows = self.editor_num_rows;
        let editor_num_columns = self.editor_num_columns;

        let row_offset = self.row_offset;
        let column_offset = self.column_offset;

        let text_num_rows = self.get_text_num_rows();

        for y in 0..editor_num_rows {
            let file_row = y + row_offset;
            if file_row >= text_num_rows {
                if text_num_rows == 0 && y == editor_num_rows / 3 {
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
                let mut row: &str = &self.rows[file_row].render;
                // Apply column offset
                if column_offset < row.len() {
                    row = &row[column_offset..];
                } else {
                    row = "";
                }

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

    fn editor_refresh_screen(&mut self) {
        self.editor_scroll();

        let mut buffer = AppendBuffer::new();

        // Hide cursor
        buffer.push("\x1b[?25l");

        // Position at the top of the screen
        buffer.push("\x1b[H");

        self.editor_draw_rows(&mut buffer);

        // Position cursor at cursor_x and cursor_y
        let cursor_x = self.cursor_x;
        let cursor_y = self.cursor_y;
        let row_offset = self.row_offset;
        let column_offset = self.column_offset;
        buffer.push(&format!(
            "\x1b[{};{}H",
            (cursor_y - row_offset) + 1,
            (cursor_x - column_offset) + 1
        ));

        // Show cursor
        buffer.push("\x1b[?25h");

        buffer.write(self);
    }

    /*** File I/O ***/
    fn editor_open(&mut self, filename: &str) {
        let file_contents = match std::fs::read_to_string(filename) {
            Ok(file) => file,
            Err(_) => {
                self.die("file read failed");
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
            self.append_row(line.to_string());
        }
    }

    /*** Input ***/
    // TODO: Refactor reading into buffer
    fn editor_read_key(&self) -> usize {
        let mut buf: [u8; 1] = [0; 1];

        if let Err(error) = io::stdin().lock().read_exact(&mut buf) {
            if error.kind() != ErrorKind::UnexpectedEof {
                self.die(&format!("Read error: {}", error));
            }
        }

        // Read escape sequences
        if buf[0] as char == '\x1b' {
            let mut seq: [u8; 3] = [0; 3];

            // Read the next two characters (if no response assume escape key)
            if let Err(error) = io::stdin().lock().read_exact(&mut seq[..1]) {
                if error.kind() == ErrorKind::UnexpectedEof {
                    return '\x1b' as usize; // Escape key
                }
                self.die(&format!("Read error: {}", error));
            }
            if let Err(error) = io::stdin().lock().read_exact(&mut seq[1..2]) {
                if error.kind() == ErrorKind::UnexpectedEof {
                    return '\x1b' as usize; // Escape key
                }
                self.die(&format!("Read error: {}", error));
            }

            if seq[0] as char == '[' {
                if seq[1] as char > '0' && seq[1] as char <= '9' {
                    if let Err(error) = io::stdin().lock().read_exact(&mut seq[2..3]) {
                        if error.kind() == ErrorKind::UnexpectedEof {
                            return '\x1b' as usize; // Escape key
                        }
                        self.die(&format!("Read error: {}", error));
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

    fn editor_move_cursor(&mut self, key: usize) {
        let on_row = self.cursor_y < self.get_text_num_rows();
        match key {
            ARROW_LEFT_KEY => {
                if self.cursor_x != 0 {
                    self.cursor_x -= 1;
                } else if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                    self.cursor_x = self.rows[self.cursor_y].render.len();
                }
            }
            ARROW_RIGHT_KEY => {
                if on_row && self.cursor_x < self.rows[self.cursor_y].render.len() {
                    self.cursor_x += 1;
                } else if on_row && self.cursor_x == self.rows[self.cursor_y].render.len() {
                    self.cursor_y += 1;
                    self.cursor_x = 0;
                }
            }
            ARROW_UP_KEY => {
                self.cursor_y = self.cursor_y.saturating_sub(1);
            }
            ARROW_DOWN_KEY => {
                if self.cursor_y < self.get_text_num_rows() {
                    self.cursor_y += 1;
                }
            }
            _ => {}
        }

        // Snap to end of line
        let current_row_len = if self.cursor_y < self.get_text_num_rows() {
            self.rows[self.cursor_y].render.len()
        } else {
            0
        };

        if self.cursor_x > current_row_len {
            self.cursor_x = current_row_len;
        }
    }

    /** Returns true if should continue */
    fn editor_process_keypress(&mut self) {
        let key: usize = self.editor_read_key();

        // Exit on q
        match key {
            _ if key == Editor::ctrl_char('q') => {
                self.cleanup();
                process::exit(0);
            }
            ARROW_LEFT_KEY | ARROW_RIGHT_KEY | ARROW_UP_KEY | ARROW_DOWN_KEY => {
                self.editor_move_cursor(key)
            }
            PAGE_DOWN_KEY | PAGE_UP_KEY => {
                let mut times = self.editor_num_rows as isize;
                while times > 0 {
                    self.editor_move_cursor(if key == PAGE_UP_KEY {
                        ARROW_UP_KEY
                    } else {
                        ARROW_DOWN_KEY
                    });
                    times -= 1;
                }
            }
            HOME_KEY => self.cursor_x = 0,
            END_KEY => {
                self.cursor_x = self.editor_num_columns - 1;
            }
            _ => {}
        };
    }
}

fn main() {
    let mut editor = Editor::new();

    editor.enable_raw_mode();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        editor.editor_open(&args[1]);
    }

    loop {
        editor.editor_refresh_screen();
        editor.editor_process_keypress();
    }
}
