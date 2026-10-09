use super::*;

/// Edge of a tile, in device pixels.
pub(super) const TILE_PX: u32 = 1024;
/// A page drawn wider than this (in device pixels) is drawn in tiles, so zooming in on a
/// high-resolution screen never asks for one enormous bitmap.
const TILING_FROM_PX: u32 = 3000;
/// Width of the whole-page picture kept under the tiles, shown stretched until they arrive.
pub(super) const BACKDROP_PX: u32 = 1600;
const TILE_LAYER_NAME: &str = "tile-layer";

pub(super) fn tiling(device_width: u32, rotation: u16) -> bool {
    device_width > TILING_FROM_PX && rotation == 0
}

/// The layer, over a page's picture, that holds the sharp tiles of the part you can see.
pub(super) fn build_tile_layer() -> gtk4::Fixed {
    let layer = gtk4::Fixed::new();
    layer.set_widget_name(TILE_LAYER_NAME);
    layer.set_can_target(false);
    layer
}

fn layer_beside(picture: &gtk4::Picture) -> Option<gtk4::Fixed> {
    let overlay = picture.parent()?;
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == TILE_LAYER_NAME {
            return widget.downcast::<gtk4::Fixed>().ok();
        }
        child = widget.next_sibling();
    }
    None
}

/// Show the tiles of `page` that are on screen (plus half a screen of margin) over `picture`,
/// asking the render thread for any it does not have yet. Removes the tiles if the page is no
/// longer drawn in tiles.
pub(super) fn sync(r: &mut ReaderState, picture: &gtk4::Picture, page: u16) {
    let Some(layer) = layer_beside(picture) else {
        return;
    };
    let scale = picture.scale_factor().max(1) as u32;
    let full_w = (READER_BASE_WIDTH * r.zoom).max(1.0) as u32 * scale;
    if !tiling(full_w, r.rotation) {
        while let Some(child) = layer.first_child() {
            layer.remove(&child);
        }
        return;
    }
    let (page_w, page_h) = r.layout_size(page);
    let full_h = (full_w as f64 * page_h as f64 / (page_w as f64).max(1.0)).round() as u32;
    let Some(viewport) = picture
        .ancestor(gtk4::ScrolledWindow::static_type())
        .and_then(|scrolled| scrolled.compute_bounds(picture))
    else {
        return;
    };
    let scale_f = scale as f64;
    let margin_x = viewport.width() as f64 / 2.0;
    let margin_y = viewport.height() as f64 / 2.0;
    let span = |origin: f32, length: f32, margin: f64, full: u32| {
        let lo = ((origin as f64 - margin) * scale_f).clamp(0.0, full as f64);
        let hi = ((origin as f64 + length as f64 + margin) * scale_f).clamp(0.0, full as f64);
        (
            (lo / TILE_PX as f64).floor() as u32,
            ((hi / TILE_PX as f64).ceil() as u32).max(1),
        )
    };
    let (col_lo, col_hi) = span(viewport.x(), viewport.width(), margin_x, full_w);
    let (row_lo, row_hi) = span(viewport.y(), viewport.height(), margin_y, full_h);
    while let Some(child) = layer.first_child() {
        layer.remove(&child);
    }
    let distance = (page as i32 - r.page as i32).unsigned_abs();
    for row in row_lo..row_hi {
        for col in col_lo..col_hi {
            let (x0, y0) = (col * TILE_PX, row * TILE_PX);
            if x0 >= full_w || y0 >= full_h {
                continue;
            }
            let key = RenderKey {
                page,
                width: full_w,
                rotation: 0,
                tone: r.tone,
                thumb: false,
                zoom_w: full_w,
                tile: Some((col as u16, row as u16)),
            };
            if let Some(texture) = r.textures.get(&key) {
                let (x1, y1) = ((x0 + TILE_PX).min(full_w), (y0 + TILE_PX).min(full_h));
                let tile = gtk4::Picture::for_paintable(&texture);
                tile.set_content_fit(gtk4::ContentFit::Fill);
                tile.set_can_shrink(true);
                let left = (x0 as f64 / scale_f).round();
                let top = (y0 as f64 / scale_f).round();
                let right = (x1 as f64 / scale_f).round();
                let bottom = (y1 as f64 / scale_f).round();
                tile.set_size_request((right - left) as i32, (bottom - top) as i32);
                layer.put(&tile, left, top);
            } else if !r.defer_renders {
                if let Some(worker) = &r.worker {
                    worker.submit(Job {
                        key,
                        priority: distance,
                    });
                }
            }
        }
    }
}

/// `sync` for a page of the continuous view, when it has a widget.
pub(super) fn sync_continuous_page(reader: &Rc<RefCell<ReaderState>>, page: u16) {
    let picture = reader
        .borrow()
        .continuous
        .as_ref()
        .and_then(|p| p.live.get(&page))
        .map(|(_, picture)| picture.clone());
    if let Some(picture) = picture {
        sync(&mut reader.borrow_mut(), &picture, page);
    }
}

/// Keep the paged view's tiles in step with its scrolling.
pub(super) fn watch_paged_scroll(
    reader: &Rc<RefCell<ReaderState>>,
    scroll: &gtk4::ScrolledWindow,
    picture: &gtk4::Picture,
) {
    for adjustment in [scroll.hadjustment(), scroll.vadjustment()] {
        let reader = reader.clone();
        let picture = picture.clone();
        adjustment.connect_value_changed(move |_| {
            let page = reader.borrow().page;
            sync(&mut reader.borrow_mut(), &picture, page);
        });
    }
}
