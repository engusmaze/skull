use std::io::{Stdout, Write};

use anyhow::Result;
use crossterm::cursor::{self, MoveTo, MoveToColumn};
use crossterm::event::KeyModifiers;
use crossterm::style::{style, Print, Stylize};
use crossterm::terminal::{Clear, ClearType, ScrollDown, ScrollUp};
use crossterm::{
    event::{read, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use crossterm::{ExecutableCommand, QueueableCommand};
use unicode_width::UnicodeWidthChar;

// Represents the final state of the editor when exiting
pub struct EditorResult {
    pub save: bool,      // Whether the user chose to save the file
    pub content: String, // The final contents of the file
}

// Main editor struct that handles the editing functionality
pub struct SkullEditor {
    lines: Vec<Vec<char>>, // Each line is stored as a vector of characters
    cursor_column: usize,  // Current cursor position within the line
    cursor_line: usize,    // Current line number
    view_pos: usize,
    current_view_height: usize,
    stdout: Stdout, // Handle to standard output for terminal manipulation
}

impl SkullEditor {
    // Creates a new editor instance from input string, splitting it into lines
    pub fn new(input: String) -> Self {
        let mut lines: Vec<Vec<char>> = input.lines().map(|line| line.chars().collect()).collect();
        // Ensure there's at least one line, even if input is empty
        if lines.len() == 0 {
            lines.push(Vec::new());
        }
        Self {
            lines,
            cursor_column: 0,
            cursor_line: 0,
            view_pos: 0,
            stdout: std::io::stdout(),
            current_view_height: 0,
        }
    }

    // Returns total number of lines in the editor
    fn get_height(&self) -> usize {
        self.lines.len()
    }

    // Returns the length of the current line
    fn get_width(&mut self) -> usize {
        self.lines[self.cursor_line].len()
    }

    fn char_width(c: char) -> usize {
        if c == '\t' {
            4
        } else {
            c.width().unwrap_or(1)
        }
    }

    fn get_cursor_offset(&self) -> usize {
        let mut real_column = 0;
        for &c in self.lines[self.cursor_line][..self.cursor_column].iter() {
            real_column += SkullEditor::char_width(c);
        }
        real_column
    }

    fn offset_to_cursor(&self, offset: usize) -> usize {
        let mut real_column = 0;
        let mut cursor_column = 0;
        for &c in self.lines[self.cursor_line].iter() {
            let width = SkullEditor::char_width(c);
            real_column += width;
            if real_column > offset {
                return cursor_column;
            }
            cursor_column += 1;
        }
        cursor_column
    }

    // Moves cursor left, wrapping to previous line if at start of line
    fn move_cursor_left(&mut self) {
        if self.cursor_column > 0 {
            self.cursor_column -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_column = self.get_width();
        }
    }

    // Moves cursor right, wrapping to next line if at end of line
    fn move_cursor_right(&mut self) {
        if self.cursor_column < self.get_width() {
            self.cursor_column += 1;
        } else if self.cursor_line + 1 < self.get_height() {
            self.cursor_column = 0;
            self.cursor_line += 1;
        }
    }

    // Moves cursor up one line, adjusting column position if necessary
    fn move_cursor_up(&mut self) {
        if self.cursor_line > 0 {
            let real_offset = self.get_cursor_offset();
            self.cursor_line -= 1;
            self.cursor_column = self.offset_to_cursor(real_offset);
        } else {
            self.cursor_column = 0;
        }
    }

    // Moves cursor down one line, adjusting column position if necessary
    fn move_cursor_down(&mut self) {
        if self.cursor_line + 1 < self.get_height() {
            let real_offset = self.get_cursor_offset();
            self.cursor_line += 1;
            self.cursor_column = self.offset_to_cursor(real_offset);
        } else {
            self.cursor_column = self.get_width();
        }
    }

    // Inserts a character at the current cursor position
    fn add_character(&mut self, c: char) {
        self.lines[self.cursor_line].insert(self.cursor_column, c);
        self.cursor_column += 1;
    }

    // Handles Enter key press - splits the current line at cursor position
    fn new_line(&mut self) {
        // Split current line at cursor position, taking remainder to new line
        let new_line = self.lines[self.cursor_line].split_off(self.cursor_column);
        self.cursor_line += 1;
        self.cursor_column = 0;
        self.lines.insert(self.cursor_line, new_line);
    }

    // Handles backspace - removes character before cursor
    fn erase_character(&mut self) {
        if self.cursor_column > 0 {
            self.cursor_column -= 1;
            self.lines[self.cursor_line].remove(self.cursor_column);
        } else if self.cursor_line > 0 {
            // If at start of line, join with previous line
            let removed_line = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            let current_line = &mut self.lines[self.cursor_line];
            current_line.extend_from_slice(&removed_line);
            self.cursor_column = current_line.len();
        }
    }

    // Redraws the entire editor contents with line numbers
    fn redraw(&mut self) -> Result<()> {
        let (_, height) = crossterm::terminal::size()?;
        let doc_height = self.get_height();
        let view_height = (height as usize).min(doc_height).max(1);

        if view_height != self.current_view_height {
            if view_height > self.current_view_height {
                self.stdout.execute(ScrollUp(cursor::position()?.1))?;
            } else {
                self.stdout.execute(ScrollDown(cursor::position()?.1))?;
            }
            self.current_view_height = view_height;
        }

        if self.view_pos > self.cursor_line {
            let diff = self.view_pos - self.cursor_line;
            self.view_pos -= diff;
        }
        if self.view_pos + view_height > doc_height {
            self.view_pos -= (self.view_pos + view_height) - doc_height;
        }
        if self.view_pos + view_height - 1 < self.cursor_line {
            let diff = self.cursor_line - (self.view_pos + view_height - 1);
            self.view_pos += diff;
        }

        // Ensure cursor column doesn't exceed the line length
        self.cursor_column = self.cursor_column.min(self.get_width());

        self.stdout
            .queue(MoveTo(0, 0))?
            .queue(Clear(ClearType::FromCursorDown))?;

        // Calculate width needed for line numbers
        let line_number_offset = self.get_height().ilog10() as usize + 1;

        // Draw each line with line number
        for (i, line) in self.lines[self.view_pos..self.view_pos + view_height]
            .iter()
            .enumerate()
        {
            if i > 0 {
                self.stdout.queue(Print("\r\n"))?;
            }
            let line_number = self.view_pos + i + 1;
            // Right-align line numbers with proper spacing
            self.stdout
                .queue(MoveToColumn(
                    (line_number_offset - line_number.ilog10() as usize) as u16,
                ))?
                .queue(Print(style(line_number).dark_grey().dim()))?
                .queue(Print(' '))?;

            // Draw the line content, handling tabs specially
            for &c in line.iter() {
                if c == '\t' {
                    self.stdout.queue(Print(style("    ").dark_grey().dim()))?;
                    continue;
                }
                let mut bytes = [0u8; 4];
                let utf8 = c.encode_utf8(&mut bytes);
                self.stdout.write_all(utf8.as_bytes())?;
            }
        }

        // Calculate and set actual cursor position, accounting for tabs
        self.stdout.queue(MoveTo(
            (line_number_offset + 2 + self.get_cursor_offset()) as u16,
            (self.cursor_line - self.view_pos) as u16,
        ))?;
        self.stdout.flush()?;
        Ok(())
    }

    // Main editor loop that handles user input
    pub fn run(mut self) -> Result<EditorResult> {
        enable_raw_mode()?; // Enable raw mode for direct terminal input

        self.redraw()?;

        let mut save = false;

        // Main event loop
        loop {
            let event = read()?;

            if let Event::Key(key_event) = event {
                match key_event.code {
                    KeyCode::Backspace => self.erase_character(),
                    KeyCode::Enter => self.new_line(),
                    KeyCode::Left => self.move_cursor_left(),
                    KeyCode::Right => self.move_cursor_right(),
                    KeyCode::Up => self.move_cursor_up(),
                    KeyCode::Down => self.move_cursor_down(),
                    // Ctrl+H acts as backspace
                    KeyCode::Char('h') if key_event.modifiers == KeyModifiers::CONTROL => {
                        self.erase_character()
                    }
                    // Ctrl+S triggers save
                    KeyCode::Char('s') if key_event.modifiers == KeyModifiers::CONTROL => {
                        save = true;
                        break;
                    }
                    KeyCode::Char(c) => self.add_character(c),
                    KeyCode::Tab => self.add_character('\t'),
                    KeyCode::Esc => {
                        break;
                    }
                    _ => {}
                }
            }

            if matches!(event, Event::Resize(..) | Event::Key(..)) {
                self.redraw()?;
            }
        }

        // Clear screen when exiting
        self.stdout
            .queue(MoveTo(0, 0))?
            .queue(Clear(ClearType::FromCursorDown))?
            .flush()?;

        // If not already saving, ask user if they want to save
        if !save {
            self.stdout
                .execute(Print("Do you want to save a file?\r\nSelect y[es]/n[o]"))?;
            loop {
                let Event::Key(key_event) = read()? else {
                    continue;
                };
                match key_event.code {
                    KeyCode::Char('y') => {
                        save = true;
                        break;
                    }
                    KeyCode::Char('n') => {
                        save = false;
                        break;
                    }
                    _ => {}
                }
            }

            self.stdout
                .queue(MoveTo(0, 0))?
                .queue(Clear(ClearType::FromCursorDown))?
                .flush()?;
        }

        disable_raw_mode()?; // Restore terminal to normal mode

        // Convert the editor contents back to a single string
        let mut content = String::new();
        for (i, line) in self.lines.into_iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.extend(line);
        }
        Ok(EditorResult { save, content })
    }
}
