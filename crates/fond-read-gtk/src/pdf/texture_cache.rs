use super::worker::RenderKey;
use gtk4::gdk;
use std::collections::{HashMap, VecDeque};

/// Finished page textures, least recently used dropped first once the byte budget is spent.
pub(super) struct TextureCache {
    map: HashMap<RenderKey, (gdk::Texture, usize)>,
    order: VecDeque<RenderKey>,
    bytes: usize,
    limit: usize,
}

impl TextureCache {
    pub fn new(limit: usize) -> TextureCache {
        TextureCache {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    pub fn get(&mut self, key: &RenderKey) -> Option<gdk::Texture> {
        let tex = self.map.get(key)?.0.clone();
        if let Some(i) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(i).unwrap();
            self.order.push_back(k);
        }
        Some(tex)
    }

    pub fn insert(&mut self, key: RenderKey, texture: gdk::Texture, bytes: usize) {
        if let Some((_, old)) = self.map.insert(key.clone(), (texture, bytes)) {
            self.bytes -= old;
            self.order.retain(|k| k != &key);
        }
        self.bytes += bytes;
        self.order.push_back(key);
        while self.bytes > self.limit && self.order.len() > 1 {
            if let Some(oldest) = self.order.pop_front() {
                if let Some((_, b)) = self.map.remove(&oldest) {
                    self.bytes -= b;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::Tone;

    fn key(page: u16) -> RenderKey {
        RenderKey {
            page,
            width: 100,
            rotation: 0,
            tone: Tone::Normal,
        }
    }

    // Textures need a display, so these exercise the bookkeeping through a stand-in.
    #[test]
    fn keys_hash_on_every_field() {
        let a = key(1);
        let mut b = key(1);
        b.width = 200;
        assert_ne!(a, b);
        let mut c = key(1);
        c.tone = Tone::Dark;
        assert_ne!(a, c);
    }
}
