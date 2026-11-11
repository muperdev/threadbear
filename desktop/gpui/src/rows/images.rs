// Pictures in the transcript: RowRenderer.imageFrames and ImageLayout. Laid out
// to the reading column's width as the Swift app does, so a grid of mixed
// shapes lines up at one height per row.
use super::metrics::*;
use super::row_wrap;
use crate::style::{MAX_CONTENT_WIDTH, SIDE_INSET};
use crate::thread::ThreadPane;
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use std::path::PathBuf;
use threadbear_core::reducer::{ImageRef, Row};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// `ImageLayout.frames`: each image fits within `cell_width` by `max_height`,
/// never scaled up. Images sharing a row are brought to that row's shortest
/// height, keeping their shapes, so their tops and bottoms line up.
pub(crate) fn image_frames(sizes: &[(f32, f32)], max_width: f32, max_height: f32, cell_width: f32, gap: f32, align_right: bool) -> (Vec<Frame>, f32) {
    const MIN_SIDE: f32 = 24.;
    if sizes.is_empty() {
        return (vec![], 0.);
    }
    let cell_width = max_width.min(cell_width);
    let fitted: Vec<(f32, f32)> = sizes
        .iter()
        .map(|&(w, h)| {
            let scale = 1f32.min(max_height / h.max(1.)).min(cell_width / w.max(1.));
            (MIN_SIDE.max((w * scale).floor()), MIN_SIDE.max((h * scale).floor()))
        })
        .collect();
    // Rows of indices, filled left to right.
    let mut rows: Vec<Vec<usize>> = vec![];
    let mut x = 0.;
    for (index, &(w, _)) in fitted.iter().enumerate() {
        if rows.is_empty() || (x > 0. && x + w > max_width) {
            rows.push(vec![]);
            x = 0.;
        }
        rows.last_mut().unwrap().push(index);
        x += w + gap;
    }
    let mut frames = vec![Frame { x: 0., y: 0., w: 0., h: 0. }; sizes.len()];
    let mut y = 0.;
    for row in &rows {
        let height = if row.len() > 1 { row.iter().map(|&i| fitted[i].1).fold(f32::MAX, f32::min) } else { fitted[row[0]].1 };
        let mut x = 0.;
        for &index in row {
            let (w, h) = fitted[index];
            let width = if row.len() > 1 && h > height { MIN_SIDE.max((w * height / h).floor()) } else { w };
            frames[index] = Frame { x, y, w: width, h: height };
            x += width + gap;
        }
        if align_right {
            if let Some(&last) = row.last() {
                let shift = max_width - (frames[last].x + frames[last].w);
                for &index in row {
                    frames[index].x += shift;
                }
            }
        }
        y += height + gap;
    }
    (frames, y - gap)
}

/// An image's size in pixels, or the Swift app's stand-in while it's unknown.
fn pixel_size(image: &ImageRef) -> (f32, f32) {
    match (image.width, image.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w as f32, h as f32),
        _ => (160., 120.),
    }
}

impl ThreadPane {
    /// The reading column's width, from the list's last layout (`render` keeps
    /// it, as the list can't be asked while it lays its rows out).
    fn column_width(&self) -> f32 {
        if self.rows.list_width <= 0. {
            return MAX_CONTENT_WIDTH;
        }
        (self.rows.list_width - SIDE_INSET * 2.).min(MAX_CONTENT_WIDTH).max(200.)
    }

    fn image_grid(&self, images: &[ImageRef], frames: Vec<Frame>, height: f32, width: f32) -> impl IntoElement {
        let mut grid = div().relative().h(px(height)).w(px(width)).flex_none();
        let all: Vec<ImageRef> = images.to_vec();
        for (i, (image, frame)) in images.iter().zip(frames).enumerate() {
            let path = image.path.clone();
            let tile_images = all.clone();
            grid = grid.child(
                div()
                    .id(("image", i))
                    .absolute()
                    .left(px(frame.x))
                    .top(px(frame.y))
                    .w(px(frame.w))
                    .h(px(frame.h))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .tooltip(super::tip(crate::viewer::TILE_TIP))
                    // The in-app viewer, not the file's default app: only a
                    // real picture opens, and nothing is ever run (viewer.rs).
                    .on_click(move |_, window, cx| crate::viewer::open_from_tile(&tile_images, i, window, cx))
                    .child(img(PathBuf::from(path)).w(px(frame.w)).h(px(frame.h)).rounded(px(8.)).object_fit(ObjectFit::Cover)),
            );
        }
        grid
    }

    /// A tool's pictures under its row: screenshots stay thumbnails, a
    /// picture the agent made is the point of the step, so it's shown large.
    pub(crate) fn images_row(&self, row: &Row, images: &[ImageRef], generated: bool) -> AnyElement {
        let width = self.column_width() - if row.in_group { GROUP_INDENT } else { 0. };
        let sizes: Vec<_> = images.iter().map(pixel_size).collect();
        let (frames, height) = if generated {
            image_frames(&sizes, width, GENERATED_MAX_SIDE, GENERATED_MAX_SIDE, IMAGE_GAP, false)
        } else {
            image_frames(&sizes, width, GRID_MAX_HEIGHT, GRID_MAX_WIDTH, IMAGE_GAP, false)
        };
        row_wrap(row, div().pt(px(ROW_TOP + 2.)).pb(px(ROW_BOTTOM)).child(self.image_grid(images, frames, height, width)))
    }

    /// Attachments sit above the bubble, right-aligned, with no box.
    pub(crate) fn user_images(&self, images: &[ImageRef]) -> AnyElement {
        let width = (self.column_width() * BUBBLE_SHARE).floor();
        let sizes: Vec<_> = images.iter().map(pixel_size).collect();
        let (frames, height) = image_frames(&sizes, width, USER_THUMB_HEIGHT, GRID_MAX_WIDTH, IMAGE_GAP, true);
        div().mb(px(IMAGE_GAP)).child(self.image_grid(images, frames, height, width)).into_any_element()
    }
}
