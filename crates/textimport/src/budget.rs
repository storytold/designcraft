use crate::ImportError;

const MAX_IMPORTED_TEXT_BYTES: usize = 64 * 1024 * 1024;
const MAX_IMPORTED_ITEMS: usize = 100_000;

#[derive(Clone, Copy)]
pub(crate) struct OutputLimits {
    pub(crate) text_bytes: usize,
    pub(crate) items: usize,
}

pub(crate) const OUTPUT_LIMITS: OutputLimits = OutputLimits { text_bytes: MAX_IMPORTED_TEXT_BYTES, items: MAX_IMPORTED_ITEMS };

pub(crate) struct OutputBudget {
    limits: OutputLimits,
    text_bytes: usize,
    items: usize,
}

impl OutputBudget {
    pub(crate) fn new(limits: OutputLimits) -> Self {
        Self { limits, text_bytes: 0, items: 0 }
    }

    pub(crate) fn add_text(&mut self, bytes: usize) -> Result<(), ImportError> {
        self.text_bytes = self.text_bytes.checked_add(bytes).ok_or_else(|| ImportError::Corrupt("imported text size overflow".into()))?;
        if self.text_bytes > self.limits.text_bytes {
            return Err(ImportError::Corrupt(format!("imported text exceeds the {}-byte limit", self.limits.text_bytes)));
        }
        Ok(())
    }

    pub(crate) fn add_items(&mut self, count: usize) -> Result<(), ImportError> {
        self.items = self.items.checked_add(count).ok_or_else(|| ImportError::Corrupt("imported item count overflow".into()))?;
        self.check_items()
    }

    /// Account for a rectangular result that can contain more empty cells than source cells.
    pub(crate) fn ensure_items(&mut self, count: usize) -> Result<(), ImportError> {
        self.items = self.items.max(count);
        self.check_items()
    }

    fn check_items(&self) -> Result<(), ImportError> {
        if self.items > self.limits.items {
            return Err(ImportError::Corrupt(format!("import produces more than {} items", self.limits.items)));
        }
        Ok(())
    }
}
