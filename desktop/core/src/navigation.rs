// Back and forward through the places Mike has been in the window, as a
// browser keeps them: going somewhere new drops whatever was ahead. A port of
// ThreadbearCore's NavigationHistory; `Place` is the app's (the sidebar view,
// the thread, the page), this only keeps the order.

#[derive(Clone, Debug)]
pub struct NavigationHistory<Place> {
    pub entries: Vec<Place>,
    /// The entry showing now; -1 while there are none.
    pub index: isize,
    pub limit: usize,
}

impl<Place: PartialEq + Clone> Default for NavigationHistory<Place> {
    fn default() -> Self {
        Self::new(100)
    }
}

impl<Place: PartialEq + Clone> NavigationHistory<Place> {
    pub fn new(limit: usize) -> Self {
        Self { entries: Vec::new(), index: -1, limit }
    }

    pub fn current(&self) -> Option<&Place> {
        if self.index < 0 {
            return None;
        }
        self.entries.get(self.index as usize)
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index < self.entries.len() as isize - 1
    }

    /// Notes where the window is now. Being where it already is (as after
    /// going back, once the app has caught up) adds nothing.
    pub fn record(&mut self, place: Place) {
        if self.current() == Some(&place) {
            return;
        }
        self.entries.truncate((self.index + 1) as usize);
        self.entries.push(place);
        if self.entries.len() > self.limit {
            let over = self.entries.len() - self.limit;
            self.entries.drain(..over);
        }
        self.index = self.entries.len() as isize - 1;
    }

    /// Puts `place` where the current entry is, for a place that's turned into
    /// another: the new-thread screen once its thread has started. Back from it
    /// then goes to wherever was before, not to what it was. Like going
    /// somewhere new, it drops whatever was ahead.
    pub fn replace_current(&mut self, place: Place) {
        if self.current().is_none() {
            self.record(place);
            return;
        }
        self.entries.truncate((self.index + 1) as usize);
        let at = self.index as usize;
        self.entries[at] = place;
        // Never the same place twice in a row, as `record` keeps it.
        if at > 0 && self.entries[at - 1] == self.entries[at] {
            self.entries.pop();
            self.index -= 1;
        }
    }

    /// The nearest earlier place that `is_valid` still allows (a thread that's
    /// been deleted, say, is skipped), or nothing with nowhere to go.
    pub fn back(&mut self, is_valid: impl Fn(&Place) -> bool) -> Option<Place> {
        self.step(-1, is_valid)
    }

    pub fn forward(&mut self, is_valid: impl Fn(&Place) -> bool) -> Option<Place> {
        self.step(1, is_valid)
    }

    /// Where `back` or `forward` would go, without going: what the bar's
    /// tooltips name.
    pub fn peek(&self, back: bool, is_valid: impl Fn(&Place) -> bool) -> Option<Place> {
        let mut copy = self.clone();
        if back {
            copy.back(is_valid)
        } else {
            copy.forward(is_valid)
        }
    }

    fn step(&mut self, delta: isize, is_valid: impl Fn(&Place) -> bool) -> Option<Place> {
        let mut next = self.index + delta;
        while next >= 0 && (next as usize) < self.entries.len() {
            if is_valid(&self.entries[next as usize]) {
                self.index = next;
                return Some(self.entries[next as usize].clone());
            }
            next += delta;
        }
        None
    }
}

/// What the ‹ › buttons' tips say (`HistoryText` in HistoryButtons.swift).
pub fn history_tip(back: bool, to: Option<&str>, shortcut: Option<&str>) -> String {
    let way = if back { "Back" } else { "Forward" };
    let mut text = match to {
        Some(title) => format!("{way} to {title}"),
        None if back => "Nothing to go back to".to_string(),
        None => "Nothing to go forward to".to_string(),
    };
    if let Some(shortcut) = shortcut {
        text.push_str(&format!(" ({shortcut})"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(place: &str) -> String {
        place.to_string()
    }

    #[test]
    fn back_and_forward() {
        let mut h = NavigationHistory::<String>::default();
        assert!(!h.can_go_back());
        for p in ["a", "b", "c"] {
            h.record(s(p));
        }
        assert_eq!(h.back(|_| true), Some(s("b")));
        assert_eq!(h.back(|_| true), Some(s("a")));
        assert_eq!(h.back(|_| true), None, "nothing before the first");
        assert_eq!(h.current(), Some(&s("a")));
        assert_eq!(h.forward(|_| true), Some(s("b")));
        assert_eq!(h.forward(|_| true), Some(s("c")));
        assert_eq!(h.forward(|_| true), None);
    }

    #[test]
    fn recording_from_the_middle_drops_what_was_ahead() {
        let mut h = NavigationHistory::<String>::default();
        for p in ["a", "b", "c"] {
            h.record(s(p));
        }
        h.back(|_| true);
        h.record(s("b"));
        assert!(h.can_go_forward(), "being where it already is adds nothing");
        h.record(s("d"));
        assert_eq!(h.entries, vec![s("a"), s("b"), s("d")]);
        assert!(!h.can_go_forward());
    }

    #[test]
    fn replacing_the_current_place() {
        let mut h = NavigationHistory::<String>::default();
        for p in ["a", "new", "b"] {
            h.record(s(p));
        }
        h.back(|_| true);
        h.replace_current(s("c"));
        assert_eq!(h.entries, vec![s("a"), s("c")]);
        assert!(!h.can_go_forward());
        assert_eq!(h.back(|_| true), Some(s("a")));
        // Turning into the place before it leaves just the one.
        h.record(s("new"));
        h.replace_current(s("a"));
        assert_eq!(h.entries, vec![s("a")]);
        // With nothing yet it's the first entry.
        let mut empty = NavigationHistory::<String>::default();
        empty.replace_current(s("a"));
        assert_eq!(empty.entries, vec![s("a")]);
    }

    #[test]
    fn skips_places_that_are_gone() {
        let mut h = NavigationHistory::<String>::default();
        for p in ["a", "gone", "c"] {
            h.record(s(p));
        }
        assert_eq!(h.back(|p| p != "gone"), Some(s("a")));
        assert_eq!(h.forward(|p| p != "gone"), Some(s("c")));
    }

    #[test]
    fn the_tip_names_where_it_goes() {
        assert_eq!(history_tip(true, Some("Morning brief"), Some("⌘[")), "Back to Morning brief (⌘[)");
        assert_eq!(history_tip(true, None, Some("⌘[")), "Nothing to go back to (⌘[)");
        assert_eq!(history_tip(false, None, None), "Nothing to go forward to");
    }
}
