use std::{
    fs::OpenOptions,
    io::{self, ErrorKind, Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    process,
    time::Instant,
};
use termios::*;

mod error;
pub mod prelude;

/*** Constants ***/
const KILO_VERSION: &str = "0.0.1";
const KILO_TAB_STOP: usize = 8;
const KILO_MESSAGE_BAR_HEIGHT: usize = 2;
const KILO_QUIT_TIMES: usize = 3;

// Editor Keys
const CARRIAGE_RETURN_KEY: usize = 13;
const BACKSPACE_KEY: usize = 127;
const ARROW_LEFT_KEY: usize = 1000;
const ARROW_RIGHT_KEY: usize = 1001;
const ARROW_UP_KEY: usize = 1002;
const ARROW_DOWN_KEY: usize = 1003;
const PAGE_UP_KEY: usize = 1004;
const PAGE_DOWN_KEY: usize = 1005;
const HOME_KEY: usize = 1006;
const END_KEY: usize = 1007;
const DELETE_KEY: usize = 1008;
const ESCAPE_KEY: usize = '\x1b' as usize;

struct EditorRow {
    chars: String,
    render: String,
}

impl EditorRow {
    pub fn new(chars: String) -> Self {
        let mut row = EditorRow {
            chars,
            render: String::new(),
        };

        row.update_render();

        row
    }

    pub fn len(&self) -> usize {
        self.chars.len()
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
        let mut index = 0;
        for c in self.chars.chars() {
            if c == '\t' {
                render.push(' ');
                index += 1;
                while index % KILO_TAB_STOP != 0 {
                    render.push(' ');
                    index += 1;
                }
            } else {
                render.push(c);
                index += 1;
            }
        }

        self.render = render;
    }

    pub fn cursor_x_to_render_cursor_x(&self, cursor_x: usize) -> usize {
        let mut render_cursor_x = 0;
        for c in self.chars.chars().take(cursor_x) {
            if c == '\t' {
                render_cursor_x += KILO_TAB_STOP - 1 - (render_cursor_x % KILO_TAB_STOP);
            }
            render_cursor_x += 1;
        }

        render_cursor_x
    }

    pub fn insert_char(&mut self, at: usize, c: char) {
        self.chars.insert(at, c);
        self.update_render();
    }

    pub fn delete_char(&mut self, at: usize) {
        self.chars.remove(at);
        self.update_render();
    }

    pub fn append_string(&mut self, s: &str) {
        self.chars.push_str(s);
        self.update_render();
    }

    pub fn split_off(&mut self, at: usize) -> String {
        let split = self.chars.split_off(at);
        self.update_render();
        split
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
    render_cursor_x: usize,
    row_offset: usize,
    column_offset: usize,
    screen_num_rows: usize,
    screen_num_columns: usize,
    rows: Vec<EditorRow>,
    dirty: usize,
    quit_times: usize,
    filename: Option<String>,
    status_message: Option<String>,
    status_message_time: Instant,
    original_terminal: Option<Termios>,
}

impl Editor {
    pub fn new() -> Self {
        let mut editor = Self {
            cursor_x: 0,
            cursor_y: 0,
            render_cursor_x: 0,
            row_offset: 0,
            column_offset: 0,
            screen_num_rows: 0,
            screen_num_columns: 0,
            rows: vec![],
            dirty: 0,
            quit_times: KILO_QUIT_TIMES,
            filename: None,
            status_message: None,
            status_message_time: Instant::now(),
            original_terminal: None,
        };

        editor.get_dimensions();

        editor
    }

    pub fn editor_insert_row(&mut self, at: usize, row: String) {
        if at > self.get_num_rows() {
            return;
        }

        let editor_row = EditorRow::new(row);
        self.rows.insert(at, editor_row);
        self.dirty += 1;
    }

    pub fn set_original_terminal(&mut self, original_terminal: Termios) {
        self.original_terminal = Some(original_terminal);
    }

    pub fn get_dimensions(&mut self) {
        if let Some((num_columns, num_rows)) = term_size::dimensions() {
            self.screen_num_rows = num_rows - KILO_MESSAGE_BAR_HEIGHT;
            self.screen_num_columns = num_columns;
        } else {
            self.die("get dimensions");
        }
    }

    pub fn get_num_rows(&self) -> usize {
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

    fn editor_draw_status_bar(&self, buffer: &mut AppendBuffer) {
        buffer.push("\x1b[7m");

        let mut truncated_filename = self.filename.as_deref().unwrap_or("[No Name]");
        if truncated_filename.len() > 20 {
            truncated_filename = &truncated_filename[..20];
        }

        let mut status = format!(
            "{} - {} lines {}",
            truncated_filename,
            self.get_num_rows(),
            if self.dirty != 0 { "(modified)" } else { "" }
        );

        let r_status = format!("{}/{}", self.cursor_y + 1, self.get_num_rows());

        if status.len() > self.screen_num_columns {
            status = status[..self.screen_num_columns].to_string();
        }
        while status.len() < self.screen_num_columns {
            if self.screen_num_columns - status.len() == r_status.len() {
                status.push_str(&r_status);
                break;
            }
            status.push(' ');
        }

        buffer.push(&status);

        buffer.push("\x1b[m");
        buffer.push("\r\n");
    }

    fn editor_draw_message_bar(&self, buffer: &mut AppendBuffer) {
        buffer.push("\x1b[K");
        let mut msg = self.status_message.as_deref().unwrap_or("");
        if msg.len() > self.screen_num_columns {
            msg = &msg[..self.screen_num_columns];
        }

        if (Instant::now() - self.status_message_time).as_secs() < 5 {
            buffer.push(msg);
        }
    }

    /*** Editor ***/
    fn editor_scroll(&mut self) {
        self.render_cursor_x = 0;
        if self.cursor_y < self.get_num_rows() {
            self.render_cursor_x =
                self.rows[self.cursor_y].cursor_x_to_render_cursor_x(self.cursor_x);
        }

        // Row offset
        if self.cursor_y < self.row_offset {
            self.row_offset = self.cursor_y;
        }

        if self.cursor_y >= self.row_offset + self.screen_num_rows {
            self.row_offset = self.cursor_y - self.screen_num_rows + 1;
        }

        // Column offset
        if self.render_cursor_x < self.column_offset {
            self.column_offset = self.render_cursor_x;
        }

        if self.render_cursor_x >= self.column_offset + self.screen_num_columns {
            self.column_offset = self.render_cursor_x - self.screen_num_columns + 1;
        }
    }

    /** Requires a flush to be guaranteed on the screen */
    fn editor_draw_rows(&self, buffer: &mut AppendBuffer) {
        let editor_num_rows = self.screen_num_rows;
        let editor_num_columns = self.screen_num_columns;

        let row_offset = self.row_offset;
        let column_offset = self.column_offset;

        let num_rows = self.get_num_rows();

        for y in 0..editor_num_rows {
            let file_row = y + row_offset;
            if file_row >= num_rows {
                if num_rows == 0 && y == editor_num_rows / 3 {
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
            buffer.push("\r\n");
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
        self.editor_draw_status_bar(&mut buffer);
        self.editor_draw_message_bar(&mut buffer);

        // Position cursor at cursor_x and cursor_y
        buffer.push(&format!(
            "\x1b[{};{}H",
            (self.cursor_y - self.row_offset) + 1,
            (self.render_cursor_x - self.column_offset) + 1
        ));

        // Show cursor
        buffer.push("\x1b[?25h");

        buffer.write(self);
    }

    fn editor_set_status_message(&mut self, message: &str) {
        self.status_message = Some(message.to_string());
        self.status_message_time = Instant::now();
    }

    /*** File I/O ***/
    fn editor_open(&mut self, filename: &str) {
        let file_contents = match std::fs::read_to_string(filename) {
            Ok(file) => file,
            Err(error) => {
                if error.kind() == ErrorKind::NotFound {
                    String::new()
                } else {
                    self.die(&format!("file read failed {}", error.kind()));
                    return;
                }
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

            self.editor_insert_row(self.get_num_rows(), line.to_string());
        }

        self.filename = Some(filename.to_string());
        self.dirty = 0;
    }

    fn editor_save(&mut self) {
        if self.filename.is_none() {
            self.editor_set_status_message("No file open to save");
        }

        let buf = self.editor_rows_to_string();

        let num_new_lines = buf.chars().filter(|&c| c == '\n').count();

        match OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .mode(0o644)
            .open(self.filename.as_ref().unwrap())
        {
            Ok(mut file) => match file.write_all(buf.as_bytes()) {
                Ok(_) => {
                    self.editor_set_status_message(&format!(
                        "{} bytes written to disk {num_new_lines}",
                        buf.len()
                    ));
                    self.dirty = 0;
                }
                Err(error) => {
                    self.editor_set_status_message(&format!("Error saving file: {}", error))
                }
            },
            Err(error) => self.editor_set_status_message(&format!("Error opening file: {}", error)),
        }
    }

    fn editor_rows_to_string(&self) -> String {
        let mut buf = String::new();
        let rows_len = self.rows.len();
        for (idx, row) in self.rows.iter().enumerate() {
            buf.push_str(&row.chars);

            if idx < rows_len - 1 {
                buf.push('\n');
            }
        }

        buf
    }

    /*** Editor operations ***/
    fn editor_insert_char(&mut self, c: char) {
        if self.cursor_y == self.get_num_rows() {
            self.editor_insert_row(self.get_num_rows(), String::new());
        }

        let row = &mut self.rows[self.cursor_y];
        row.insert_char(self.cursor_x, c);
        self.cursor_x += 1;
        self.dirty += 1;
    }

    fn editor_insert_newline(&mut self) {
        if self.cursor_x == 0 {
            self.editor_insert_row(self.cursor_y, String::new());
        } else {
            let row = &mut self.rows[self.cursor_y];
            let new_row = row.split_off(self.cursor_x);
            self.editor_insert_row(self.cursor_y + 1, new_row);
            self.dirty += 1;
        }

        self.cursor_y += 1;
        self.cursor_x = 0;
    }

    fn editor_delete_row(&mut self, at: usize) -> Option<EditorRow> {
        if at >= self.get_num_rows() {
            return None;
        }

        self.dirty += 1;
        Some(self.rows.remove(at))
    }

    fn editor_delete_char(&mut self) {
        if self.cursor_y == self.screen_num_rows {
            return;
        };
        if self.cursor_x == 0 && self.cursor_y == 0 {
            return;
        }

        if self.cursor_x > 0 {
            let row = &mut self.rows[self.cursor_y];
            row.delete_char(self.cursor_x - 1);
            self.cursor_x -= 1;
            self.dirty += 1;
        } else {
            self.cursor_x = self.rows[self.cursor_y - 1].len();
            let deleted_row = self.editor_delete_row(self.cursor_y);
            if let Some(row) = deleted_row {
                self.rows[self.cursor_y - 1].append_string(&row.chars);
            }

            self.cursor_y -= 1;
            // Dirty is incremented in editor_delete_row
        }
    }

    /*** Input ***/
    // TODO: Refactor reading into buffer
    fn editor_read_key(&self) -> usize {
        let mut buf: [u8; 1] = [0; 1];

        while if let Err(error) = io::stdin().lock().read_exact(&mut buf) {
            if error.kind() != ErrorKind::UnexpectedEof {
                self.die(&format!("Read error: {}", error));
            }
            true
        } else {
            false // Break loop
        } {
            continue;
        }

        // Read escape sequences
        if buf[0] as usize == ESCAPE_KEY {
            let mut seq: [u8; 3] = [0; 3];

            // Read the next two characters (if no response assume escape key)
            if let Err(error) = io::stdin().lock().read_exact(&mut seq[..1]) {
                if error.kind() == ErrorKind::UnexpectedEof {
                    return ESCAPE_KEY;
                }
                self.die(&format!("Read error: {}", error));
            }
            if let Err(error) = io::stdin().lock().read_exact(&mut seq[1..2]) {
                if error.kind() == ErrorKind::UnexpectedEof {
                    return ESCAPE_KEY;
                }
                self.die(&format!("Read error: {}", error));
            }

            if seq[0] as char == '[' {
                if seq[1] as char > '0' && seq[1] as char <= '9' {
                    if let Err(error) = io::stdin().lock().read_exact(&mut seq[2..3]) {
                        if error.kind() == ErrorKind::UnexpectedEof {
                            return ESCAPE_KEY;
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

            return ESCAPE_KEY;
        }

        buf[0] as usize
    }

    fn editor_move_cursor(&mut self, key: usize) {
        let on_row = self.cursor_y < self.get_num_rows();
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
                if self.cursor_y < self.get_num_rows() {
                    self.cursor_y += 1;
                }
            }
            _ => {}
        }

        // Snap to end of line
        let current_row_len = if self.cursor_y < self.get_num_rows() {
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
                if self.dirty != 0 && self.quit_times > 0 {
                    self.editor_set_status_message(&format!(
                        "WARNING!!! File has unsaved changes. Press Ctrl-Q {} more times to quit.",
                        self.quit_times
                    ));
                    self.quit_times -= 1;
                    return;
                }
                self.cleanup();
                process::exit(0);
            }
            _ if key == Editor::ctrl_char('s') => {
                self.editor_save();
            }
            CARRIAGE_RETURN_KEY => {
                self.editor_insert_newline();
            }
            ARROW_LEFT_KEY | ARROW_RIGHT_KEY | ARROW_UP_KEY | ARROW_DOWN_KEY => {
                self.editor_move_cursor(key)
            }
            PAGE_DOWN_KEY | PAGE_UP_KEY => {
                if key == PAGE_UP_KEY {
                    self.cursor_y = self.row_offset;
                } else if key == PAGE_DOWN_KEY {
                    self.cursor_y = self.row_offset + self.screen_num_rows - 1;
                    if self.cursor_y > self.get_num_rows() {
                        self.cursor_y = self.get_num_rows();
                    }
                }

                let mut times = self.screen_num_rows;
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
                if self.cursor_y < self.get_num_rows() {
                    self.cursor_x = self.rows[self.cursor_y].len();
                }
            }
            BACKSPACE_KEY | DELETE_KEY => {
                if key == DELETE_KEY {
                    self.editor_move_cursor(ARROW_RIGHT_KEY);
                }
                self.editor_delete_char();
            }
            _ if key == Editor::ctrl_char('h') => {
                self.editor_delete_char();
            }
            ESCAPE_KEY => {
                // Do nothing
            }
            _ if key == Editor::ctrl_char('l') => {
                // Same as ESCAPE
                // Do nothing
            }
            _ => {
                if (key < 128 && (key as u8).is_ascii()) || key == '\t' as usize {
                    // Insert character
                    self.editor_insert_char(key as u8 as char);
                }
            }
        };

        self.quit_times = KILO_QUIT_TIMES;
    }
}

fn main() {
    let mut editor = Editor::new();

    editor.enable_raw_mode();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        editor.editor_open(&args[1]);
    }

    editor.editor_set_status_message("HELP: Ctrl-S = save | Ctrl-Q = quit");

    loop {
        editor.editor_refresh_screen();
        editor.editor_process_keypress();
    }
}
