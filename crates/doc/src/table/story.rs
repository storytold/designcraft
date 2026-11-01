use super::{Arc, Story, StoryId, TABLE_ANCHOR, Table};

/// Append `other` to `s` as new paragraphs.
pub(super) fn append_story(s: &mut Story, other: &Story) {
    if s.is_empty() {
        *s = Story { id: s.id, ..other.clone() };
        s.rev = s.rev.wrapping_add(1);
        return;
    }
    let base = s.len();
    s.insert(base, "\n");
    // Copy other's runs with their formats.
    for (r, f) in other.runs() {
        let at = s.len();
        s.insert_with(at, &other.text[r], f.clone());
    }
    let first_new = s.paras.len() - other.paras.len();
    for (k, p) in other.paras.iter().enumerate() {
        if let Some(dst) = s.paras.get_mut(first_new + k) {
            *dst = p.clone();
        }
    }
}

// ---------- tables in stories ----------

impl crate::Document {
    /// The story text edits go to: a cell's story when `cell` is set, else the story itself.
    pub fn text_story(&self, story: StoryId, cell: Option<crate::CellAddr>) -> Option<&Story> {
        let st = self.story(story)?;
        match cell {
            Some(c) if c.table == crate::FOOTNOTE_TABLE => st.note(c.row as u64).map(|n| &n.text),
            Some(c) => st.tables.get(&c.table)?.cell(c.row, c.col).map(|x| &x.text),
            None => Some(st),
        }
    }

    /// Mutable [`Document::text_story`] (a cell edit bumps the containing story's revision).
    pub fn text_story_mut(&mut self, story: StoryId, cell: Option<crate::CellAddr>) -> Option<&mut Story> {
        let st = self.story_mut(story)?;
        match cell {
            Some(c) if c.table == crate::FOOTNOTE_TABLE => st.note_mut(c.row as u64).map(|n| &mut n.text),
            Some(c) => st.table_mut(c.table)?.cell_mut(c.row, c.col).map(|x| &mut x.text),
            None => Some(st),
        }
    }
}

impl Story {
    /// Visit this story and every text inside it (table cells, footnotes), mutably.
    pub fn for_each_text_mut(&mut self, f: &mut impl FnMut(&mut Story)) {
        f(self);
        for t in self.tables.values_mut() {
            for c in &mut std::sync::Arc::make_mut(t).cells {
                c.text.for_each_text_mut(f);
            }
        }
        for n in &mut self.notes {
            std::sync::Arc::make_mut(n).text.for_each_text_mut(f);
        }
    }

    /// The table whose anchor paragraph is `pi`.
    pub fn para_table(&self, pi: usize) -> Option<&Arc<Table>> {
        self.tables.get(&self.paras.get(pi)?.table?)
    }

    /// Byte offset of a table's anchor character.
    pub fn table_anchor(&self, id: u64) -> Option<usize> {
        let pi = self.paras.iter().position(|p| p.table == Some(id))?;
        let r = self.para_ranges().get(pi)?.clone();
        self.text[r.clone()].find(TABLE_ANCHOR).map(|k| r.start + k)
    }

    /// The table anchored at or right before byte `pos` (a caret next to the anchor).
    pub fn table_at(&self, pos: usize) -> Option<u64> {
        let pi = self.para_at(pos);
        self.paras.get(pi)?.table
    }

    /// Mutable table (copy-on-write); bumps the story revision.
    pub fn table_mut(&mut self, id: u64) -> Option<&mut Table> {
        let t = self.tables.get_mut(&id)?;
        self.rev = self.rev.wrapping_add(1);
        Some(Arc::make_mut(t))
    }

    /// Insert `table` at `pos`, giving its anchor its own paragraph. Returns the anchor's byte offset.
    pub fn insert_table(&mut self, pos: usize, table: Table) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let pi = self.para_at(pos);
        let pstart = self.para_ranges()[pi].start;
        let mut s = String::new();
        if pos > pstart {
            s.push('\n');
        }
        let anchor = pos + s.len();
        s.push(TABLE_ANCHOR);
        if !self.text[pos..].starts_with('\n') {
            s.push('\n');
        }
        let id = table.id;
        self.tables.insert(id, Arc::new(table));
        // Insert the text first (fix-up would drop an unreferenced table), then point the paragraph at it.
        self.insert_raw(pos, &s);
        let api = self.para_at(anchor);
        self.paras[api].table = Some(id);
        self.fix_tables();
        self.rev = self.rev.wrapping_add(1);
        anchor
    }

    /// Keep paragraphs and tables consistent after an edit: a paragraph keeps its table only if it
    /// still contains an anchor character, every table is referenced once, and tables nobody
    /// references are dropped. An anchor that shares a table's paragraph with another anchor gets
    /// a paragraph of its own again, so a table paragraph always holds exactly one anchor.
    pub(crate) fn fix_tables(&mut self) {
        self.fix_tables_with(&[]);
    }

    /// [`Story::fix_tables`] for an edit that knows which table an anchor belongs to: `claims` are
    /// `(anchor byte offset, table id)` pairs that win over the paragraphs' table ids (a paragraph
    /// names one table, so merging two anchor paragraphs would otherwise lose one). Every other
    /// table takes the first unclaimed anchor of its paragraph. Returns the byte offsets, before
    /// the fix-up, where it inserted paragraph returns.
    pub(crate) fn fix_tables_with(&mut self, claims: &[(usize, u64)]) -> Vec<usize> {
        if self.tables.is_empty() && self.paras.iter().all(|p| p.table.is_none()) {
            return Vec::new();
        }
        let mut owner = std::collections::BTreeMap::new();
        let mut used = std::collections::HashSet::new();
        for &(at, id) in claims {
            if self.text.get(at..).is_some_and(|t| t.starts_with(TABLE_ANCHOR))
                && self.tables.contains_key(&id)
                && !owner.contains_key(&at)
                && used.insert(id)
            {
                owner.insert(at, id);
            }
        }
        let ranges = self.para_ranges();
        for (p, r) in self.paras.iter().zip(&ranges) {
            let Some(id) = p.table.filter(|id| self.tables.contains_key(id) && !used.contains(id)) else { continue };
            let free = self.text[r.clone()].match_indices(TABLE_ANCHOR).map(|(k, _)| r.start + k).find(|at| !owner.contains_key(at));
            if let Some(at) = free {
                used.insert(id);
                owner.insert(at, id);
            }
        }
        // A paragraph holding a table's anchor and other anchors is split before each later anchor.
        let mut splits = Vec::new();
        for r in &ranges {
            let anchors: Vec<usize> = self.text[r.clone()].match_indices(TABLE_ANCHOR).map(|(k, _)| r.start + k).collect();
            if anchors.len() > 1 && anchors.iter().any(|at| owner.contains_key(at)) {
                splits.extend(anchors.into_iter().skip(1));
            }
        }
        for &at in splits.iter().rev() {
            self.insert_raw(at, "\n");
        }
        // Each owned anchor, moved past the returns inserted before it, names its paragraph's table.
        let owner: std::collections::BTreeMap<usize, u64> =
            owner.into_iter().map(|(at, id)| (at + splits.partition_point(|&s| s <= at), id)).collect();
        let ranges = self.para_ranges();
        for (p, r) in self.paras.iter_mut().zip(ranges) {
            p.table = owner.range(r).next().map(|(_, &id)| id);
        }
        self.tables.retain(|id, _| used.contains(id));
        splits
    }

    /// Check table invariants (part of [`Story::check`]).
    pub(crate) fn check_tables(&self) -> Result<(), String> {
        let ranges = self.para_ranges();
        let mut used = std::collections::HashSet::new();
        for (pi, p) in self.paras.iter().enumerate() {
            let Some(id) = p.table else { continue };
            let t = self.tables.get(&id).ok_or_else(|| format!("story {}: paragraph {pi} points at missing table {id}", self.id.0))?;
            if t.id != id {
                return Err(format!("story {}: table key {id} != id {}", self.id.0, t.id));
            }
            if self.text[ranges[pi].clone()].matches(TABLE_ANCHOR).count() != 1 {
                return Err(format!("story {}: table {id} paragraph must hold exactly one anchor", self.id.0));
            }
            if !used.insert(id) {
                return Err(format!("story {}: table {id} anchored twice", self.id.0));
            }
            t.check()?;
        }
        if used.len() != self.tables.len() {
            return Err(format!("story {}: {} tables but {} anchored", self.id.0, self.tables.len(), used.len()));
        }
        Ok(())
    }
}
