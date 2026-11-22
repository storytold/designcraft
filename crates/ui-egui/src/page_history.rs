//! Layout ▸ Go Back / Go Forward: the pages a window has jumped between, in the order it showed them.
//! InDesign keeps this order for the session, like a web browser's Back and Forward.

use designcraft_doc::ids::PageId;
use designcraft_geom::Vec2;

/// Entries kept: the oldest go first.
const LIMIT: usize = 256;

#[derive(Clone, Debug, Default)]
pub struct PageHistory {
    /// Each page with where the view centre was on it, as an offset from the page's centre.
    pages: Vec<(PageId, Vec2)>,
    /// The entry in view.
    at: usize,
}

impl PageHistory {
    /// A jump or Go Back starts from `page`, the view centre `offset` from the page's centre: the
    /// entry in view takes that place, wherever scrolling has moved the view since it was recorded,
    /// as a browser keeps a page's scroll position.
    pub fn leave(&mut self, page: PageId, offset: Vec2) {
        match self.pages.get_mut(self.at) {
            Some(e) => *e = (page, offset),
            None => self.visit(page, offset),
        }
    }

    /// A jump has brought the view to `page`, its centre `offset` from the page's centre. Another page
    /// than the entry in view drops the entries ahead of it, as a browser does when a link is followed
    /// after going back.
    pub fn visit(&mut self, page: PageId, offset: Vec2) {
        if let Some(e) = self.pages.get_mut(self.at)
            && e.0 == page
        {
            e.1 = offset;
            return;
        }
        self.pages.truncate(self.at.saturating_add(1));
        self.pages.push((page, offset));
        if self.pages.len() > LIMIT {
            self.pages.remove(0);
        }
        self.at = self.pages.len().saturating_sub(1);
    }

    /// The entry Go Back (`step` < 0) or Go Forward goes to: the nearest one that way whose page is
    /// still in the document and isn't the page in view.
    fn target(&self, step: isize, exists: impl Fn(PageId) -> bool) -> Option<usize> {
        let current = self.pages.get(self.at).map(|e| e.0);
        let usable = |i: &usize| self.pages.get(*i).is_some_and(|&(p, _)| Some(p) != current && exists(p));
        if step < 0 { (0..self.at).rev().find(usable) } else { (self.at.saturating_add(1)..self.pages.len()).find(usable) }
    }

    /// Whether Go Back (`step` < 0) or Go Forward has a page to go to.
    pub fn can_go(&self, step: isize, exists: impl Fn(PageId) -> bool) -> bool {
        self.target(step, exists).is_some()
    }

    /// Go Back (`step` < 0) or Go Forward: the page to show and where the view was on it. It becomes
    /// the entry in view.
    pub fn go(&mut self, step: isize, exists: impl Fn(PageId) -> bool) -> Option<(PageId, Vec2)> {
        let i = self.target(step, exists)?;
        self.at = i;
        self.pages.get(i).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: fn(PageId) -> bool = |_| true;

    fn visited(pages: &[u64]) -> PageHistory {
        let mut h = PageHistory::default();
        for &p in pages {
            h.visit(PageId(p), Vec2::ZERO);
        }
        h
    }

    fn page(e: Option<(PageId, Vec2)>) -> Option<u64> {
        e.map(|(p, _)| p.0)
    }

    #[test]
    fn go_back_and_forward_retrace_the_pages_in_the_order_they_were_viewed() {
        let mut h = visited(&[1, 5, 3]);
        assert!(!h.can_go(1, ALL), "nothing ahead of the last page viewed");
        assert_eq!(page(h.go(-1, ALL)), Some(5));
        assert_eq!(page(h.go(-1, ALL)), Some(1));
        assert_eq!(h.go(-1, ALL), None, "the first page viewed has nothing before it");
        assert_eq!(page(h.go(1, ALL)), Some(5));
        assert_eq!(page(h.go(1, ALL)), Some(3));
        assert_eq!(h.go(1, ALL), None);
    }

    #[test]
    fn going_back_does_not_record_the_page_it_shows() {
        let mut h = visited(&[1, 5, 3]);
        let (p, offset) = h.go(-1, ALL).unwrap();
        // The next jump starts from it.
        h.leave(p, offset);
        assert_eq!(page(h.go(1, ALL)), Some(3), "Go Forward still has the page left");
    }

    #[test]
    fn an_entry_keeps_the_place_a_jump_left_it_at() {
        let mut h = visited(&[1, 5]);
        // Scrolled from page 5 to page 7, then jumped.
        h.leave(PageId(7), Vec2::new(-200.0, 30.0));
        h.visit(PageId(9), Vec2::ZERO);
        assert_eq!(h.go(-1, ALL), Some((PageId(7), Vec2::new(-200.0, 30.0))), "back to where the view was");
        assert_eq!(page(h.go(-1, ALL)), Some(1), "scrolling added no entry");
        assert_eq!(page(h.go(1, ALL)), Some(7));
        assert_eq!(page(h.go(1, ALL)), Some(9), "nor dropped the ones ahead");
    }

    #[test]
    fn viewing_another_page_after_going_back_clears_go_forward() {
        let mut h = visited(&[1, 5, 3]);
        h.go(-1, ALL);
        h.visit(PageId(7), Vec2::ZERO);
        assert_eq!(page(h.go(-1, ALL)), Some(5), "the page gone back to stays behind the new one");
        assert_eq!(page(h.go(1, ALL)), Some(7));
        assert!(!h.can_go(1, ALL), "the page that was ahead is gone");
    }

    #[test]
    fn deleted_pages_are_skipped() {
        let mut h = visited(&[1, 5, 3]);
        let exists = |p: PageId| p != PageId(5);
        assert_eq!(page(h.go(-1, exists)), Some(1));
        assert_eq!(page(h.go(1, exists)), Some(3));
        assert!(!visited(&[5, 3]).can_go(-1, exists), "no page left to go back to");
    }

    #[test]
    fn the_history_is_bounded() {
        let mut h = visited(&(0..LIMIT as u64 + 10).collect::<Vec<_>>());
        let mut back = 0;
        while h.go(-1, ALL).is_some() {
            back += 1;
        }
        assert_eq!(back, LIMIT - 1);
    }
}
