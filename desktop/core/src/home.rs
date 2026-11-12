// The pages the activity bar opens beside the sidebar (Scheduled's now, the
// Librarian's next): their number tiles, and what the picture says when it's
// poked (HomePage.swift, PokeLines.swift).

/// One number on a page, what it counts, and more in its tooltip (`HomeStat`).
#[derive(Clone, Debug, PartialEq)]
pub struct HomeStat {
    pub value: String,
    pub label: String,
    pub help: String,
}

impl HomeStat {
    pub fn new(value: impl ToString, label: &str, help: &str) -> HomeStat {
        HomeStat { value: value.to_string(), label: label.to_owned(), help: help.to_owned() }
    }
}

/// What a home page's picture says when it's clicked, one line a poke in
/// turn, and something else on the fifth in a row (`PokeLines`). A secret:
/// nothing hints at it.
#[derive(Clone, Debug, PartialEq)]
pub struct PokeLines {
    lines: &'static [&'static str],
    next: usize,
    in_a_row: usize,
}

impl PokeLines {
    pub const LIBRARIAN: &'static [&'static str] = &["Shh.", "Filed under B, for bear.", "Haiku, mostly. It's cheaper.", "I've read all your threads. No comment."];
    pub const SCHEDULED: &'static [&'static str] = &["Not yet.", "Five more minutes.", "I'll remind you. It's the one thing I'm good at."];
    const FIFTH: &'static str = "Alright, alright.";

    pub fn new(lines: &'static [&'static str]) -> PokeLines {
        PokeLines { lines, next: 0, in_a_row: 0 }
    }

    pub fn poke(&mut self) -> Option<&'static str> {
        if self.lines.is_empty() {
            return None;
        }
        self.in_a_row += 1;
        if self.in_a_row == 5 {
            return Some(Self::FIFTH);
        }
        let line = self.lines[self.next];
        self.next = (self.next + 1) % self.lines.len();
        Some(line)
    }

    /// The line faded out before another poke: the next run starts counting again.
    pub fn end_run(&mut self) {
        self.in_a_row = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pokes_go_round_and_the_fifth_in_a_row_is_different() {
        let mut pokes = PokeLines::new(PokeLines::SCHEDULED);
        let said: Vec<&str> = (0..5).filter_map(|_| pokes.poke()).collect();
        assert_eq!(said, vec!["Not yet.", "Five more minutes.", "I'll remind you. It's the one thing I'm good at.", "Not yet.", "Alright, alright."]);
        pokes.end_run();
        assert_eq!(pokes.poke(), Some("Five more minutes."), "the fifth didn't use a line up");
    }
}
