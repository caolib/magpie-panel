//! Decoded model icons, keyed by their configured source.
//!
//! Icons are files the user points a model mapping at, so they may change
//! while the panel runs. Every frame re-checks the file's modification time —
//! one `metadata` call per distinct icon, not per row — and reloads only when
//! the stamp moved. Decoding is cached because a repaint draws the same icon
//! once per row.

use std::collections::HashMap;
use std::time::SystemTime;

use crate::theme::Image;

#[derive(Default)]
pub struct LogoCache {
    entries: HashMap<String, Entry>,
}

struct Entry {
    /// Modification time the cached decode came from. `None` for the built-in
    /// mark, which never changes, or for a file that could not be read.
    stamp: Option<SystemTime>,
    image: Option<Image>,
}

impl LogoCache {
    /// Reload the icons the visible rows ask for, if their files changed.
    pub fn refresh<'a>(&mut self, sources: impl Iterator<Item = &'a str>) {
        let mut seen: Vec<&str> = Vec::new();
        for source in sources {
            if source.is_empty() || seen.contains(&source) {
                continue;
            }
            seen.push(source);
            self.ensure(source);
        }
    }

    pub fn get(&self, source: &str) -> Option<&Image> {
        self.entries.get(source)?.image.as_ref()
    }

    fn ensure(&mut self, source: &str) {
        let stamp = std::fs::metadata(source)
            .and_then(|meta| meta.modified())
            .ok();
        if let Some(entry) = self.entries.get(source)
            && entry.stamp == stamp
        {
            return;
        }
        let image = std::fs::read(source)
            .ok()
            .and_then(|bytes| Image::load_png(&bytes));
        self.entries
            .insert(source.to_string(), Entry { stamp, image });
    }
}
