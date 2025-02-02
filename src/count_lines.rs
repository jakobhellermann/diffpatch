use std::fmt::Write;

pub struct CountLines<W> {
    writer: W,
    lines_written: usize,
    chars_written: usize,
    term_width: usize,

    debug_current_line: String,
}

impl<W> CountLines<W> {
    pub fn new(w: W, term_width: u16) -> Self {
        CountLines {
            writer: w,
            lines_written: 0,
            chars_written: 0,
            term_width: term_width as usize,
            debug_current_line: String::new(),
        }
    }
    pub fn take_lineno(&mut self) -> u16 {
        // let lines = self.lines_written + (self.chars_written > 0) as usize;
        if self.chars_written > 0 {
            // panic!();
        }
        let lines = self.lines_written;
        self.lines_written = 0;
        self.chars_written = 0;
        lines as u16
    }
}

impl<W: std::io::Write> std::io::Write for CountLines<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // naive:
        // self.lines_written += buf.iter().filter(|&&x| x == b'\n').count();
        // return self.writer.write(buf);

        // basic line wrapping:
        let mut after_newline = false;
        for line in buf.split(|&x| x == b'\n') {
            if after_newline {
                self.chars_written = 0;
                self.lines_written += 1;
                self.debug_current_line.clear();
            }

            self.chars_written += stripped_size(line);

            self.lines_written += (self.chars_written.saturating_sub(1)) / self.term_width;
            self.chars_written %= self.term_width;

            if self.chars_written >= self.term_width {
                self.debug_current_line.clear();
            }

            after_newline = true;
        }

        self.debug_current_line
            .write_str(std::str::from_utf8(buf).unwrap())
            .unwrap();
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[test]
fn check_size() {
    assert_eq!(stripped_size(b"\x1b[31m"), 0);
    assert_eq!(stripped_size(b"\x1b[31mhi"), 2);
    assert_eq!(stripped_size(b"\x1b[31mHello World"), 11);
    assert_eq!(stripped_size(b"\x1b[31mHello World\x1b[39m"), 11);
}
fn stripped_size(str: &[u8]) -> usize {
    let mut i = 0;
    let mut state = 0;

    for &b in str {
        state = match state {
            0 if b == b'\x1b' => 1,
            1 if b == b'[' => 2,
            2 if matches!(b, b'\x30'..=b'\x3f') => 2,
            2 | 3 if matches!(b, b'\x20'..=b'\x2f') => 3,
            2 | 3 if matches!(b, b'\x40'..=b'\x7e') => 0,
            0 => {
                i += 1;
                0
            }
            other => other,
        };
    }
    i
}
