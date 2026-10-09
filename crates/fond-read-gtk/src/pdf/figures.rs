use super::*;

/// Ask the render thread for `region` (page points as displayed) of `page`, drawn `logical_w`
/// pixels wide, and call `done` with the picture when it arrives.
pub(super) fn request(
    reader: &Rc<RefCell<ReaderState>>,
    scale: i32,
    page: u16,
    region: [f32; 4],
    logical_w: i32,
    done: Rc<dyn Fn(gdk::Texture)>,
) {
    let mut r = reader.borrow_mut();
    let Some(geom) = r.geom(page) else {
        return;
    };
    let (dw, _) = geom.display_size();
    let region_w = (region[2] - region[0]).max(1.0) as f64;
    let per_pt = logical_w as f64 * scale.max(1) as f64 / region_w;
    let zoom_w = (dw as f64 * per_pt).round() as u32;
    let key = RenderKey {
        page,
        width: zoom_w,
        rotation: 0,
        tone: match crate::typography::shared().get().theme {
            crate::typography::ReadingTheme::Light => Tone::Normal,
            crate::typography::ReadingTheme::Sepia => Tone::Sepia,
            crate::typography::ReadingTheme::Dark => Tone::Dark,
        },
        thumb: true,
        zoom_w,
        tile: None,
        crop: Some([
            (region[0] as f64 * per_pt).round() as i32,
            (region[1] as f64 * per_pt).round() as i32,
            (region_w * per_pt).round().max(1.0) as i32,
            ((region[3] - region[1]) as f64 * per_pt).round().max(1.0) as i32,
        ]),
    };
    r.figure_waiters.push((key.clone(), done));
    if let Some(worker) = &r.worker {
        worker.submit(Job { key, priority: 1 });
    }
}

/// A finished figure goes to whoever asked for it.
pub(super) fn deliver(reader: &Rc<RefCell<ReaderState>>, done: Rendered) {
    let waiting: Vec<_> = {
        let mut r = reader.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut r.figure_waiters)
            .into_iter()
            .partition(|(key, _)| *key == done.key);
        r.figure_waiters = rest;
        mine
    };
    if waiting.is_empty() {
        return;
    }
    let stride = done.width as usize * 4;
    let texture = gdk::MemoryTexture::new(
        done.width as i32,
        done.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(done.rgba),
        stride,
    );
    for (_, f) in waiting {
        f(texture.clone().upcast());
    }
}
