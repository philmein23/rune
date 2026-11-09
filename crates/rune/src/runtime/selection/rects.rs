//! `Cx::selection_rects`: the line boxes of the [`El::selectable`](crate::El::selectable)
//! text selection, for a view that places something over it (a Quote button). It is a layout
//! read like `Cx::layout`, under a key no element can have, so it settles, replays from
//! `memo` and re-runs the view the same way; the boxes ride along with the rects the view is
//! given.
use super::*;

/// The layout-read key `Cx::selection_rects` reads under.
pub(crate) const SELECTION: &str = "\u{1}selection";
/// The start of the layout-read key `Cx::text_rects` reads under; the element key and the
/// ranges follow, so a read of other ranges is another read.
pub(crate) const TEXT_RECTS: &str = "\u{1}text-rects\u{1}";

/// One list of line boxes per range of a `Cx::text_rects` read.
pub(crate) type TextBoxes = Rc<[Vec<[f32; 4]>]>;

impl<S> Cx<S> {
    /// The line boxes of the text selection, `[x, y, w, h]` each in window points after
    /// scrolling, in reading order: one per line of each selected text, as its highlight draws
    /// them. Empty (and allocating nothing) when nothing is selected or only a caret is placed.
    /// Boxes aren't clipped to their scroll viewport, so a view can tell when the selection has
    /// scrolled out of sight. Only mounted texts have boxes: the rows of a virtual list that
    /// scrolled away have none. Like [`Cx::layout`], boxes are layout points: an ancestor's
    /// [`El::scale`](crate::El::scale) isn't applied.
    ///
    /// A view that reads it re-runs when a box appears, goes, or moves by more than half a
    /// point: in the same commit when layout moved it, the next frame when the selection
    /// changed or scrolling moved it. While a drag changes the selection that's every frame,
    /// so a toolbar that waits for the release
    /// ([`SelectionChange::dragging`](crate::SelectionChange::dragging)) should only read it
    /// once shown. A `memo` that reads it is rebuilt when the boxes move.
    pub fn selection_rects(&self) -> Vec<[f32; 4]> {
        layout_read::note_read(&mut self.layout_reads.borrow_mut(), SELECTION, None);
        crate::memo::read_selection(&self.layout.selection);
        self.layout.selection.to_vec()
    }

    /// The line boxes of each byte range of the text element keyed `key` (a `text` or a
    /// `text_input`), one list per range in `ranges`, one `[x, y, w, h]` per line the range
    /// touches. Like [`Cx::layout`], boxes are window points as if every scroll viewport were at
    /// its origin, so scrolling re-runs no view. A range past the end of the text is clamped.
    ///
    /// One read costs one lookup of the element plus the ranges, whatever their number. A view
    /// that reads new ranges gets empty lists on its first pass and the boxes on the next, in
    /// the same commit. In a `text_input`, offsets are of `edit.text`. Read it outside any
    /// `memo`: a memo isn't rebuilt when these boxes move.
    pub fn text_rects(&self, key: &str, ranges: &[std::ops::Range<usize>]) -> Vec<Vec<[f32; 4]>> {
        let read = text_read_key(key, ranges);
        layout_read::note_read(&mut self.layout_reads.borrow_mut(), &read, None);
        self.layout.text.get(&read).map_or_else(|| vec![Vec::new(); ranges.len()], |b| b.to_vec())
    }
}

/// Bytes `a..b` of the text at a path.
pub(crate) type SelectedRange = (String, usize, usize);

/// Whether any box of `a` and `b` differs by more than half a point, or one has a box the
/// other lacks.
pub(crate) fn boxes_moved(a: &[[f32; 4]], b: &[[f32; 4]]) -> bool {
    a.len() != b.len() || a.iter().zip(b).any(|(x, y)| layout_read::moved(Some(*x), Some(*y)))
}

/// [`boxes_moved`] across every range of a `Cx::text_rects` read.
pub(crate) fn text_boxes_moved(a: &[Vec<[f32; 4]>], b: &[Vec<[f32; 4]>]) -> bool {
    a.len() != b.len() || a.iter().zip(b).any(|(x, y)| boxes_moved(x, y))
}

fn text_read_key(key: &str, ranges: &[std::ops::Range<usize>]) -> String {
    use std::fmt::Write;
    let mut read = format!("{TEXT_RECTS}{key}\u{1}");
    for r in ranges {
        let _ = write!(read, "{}-{},", r.start, r.end);
    }
    read
}

/// The element key and byte ranges of a `Cx::text_rects` read key.
fn parse_text_read(read: &str) -> Option<(&str, Vec<(usize, usize)>)> {
    let (key, ranges) = read.strip_prefix(TEXT_RECTS)?.split_once('\u{1}')?;
    let ranges = ranges.split_terminator(',').filter_map(|r| {
        let (a, b) = r.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });
    Some((key, ranges.collect()))
}

impl<S: 'static> Runtime<S> {
    /// The selection's line boxes laid out in the last commit's nodes.
    pub(super) fn selection_boxes(&self) -> Vec<[f32; 4]> {
        let mut out = Vec::new();
        for (i, a, b) in self.selection_ranges() { self.text_boxes(&self.nodes, i, a, b, &mut out); }
        out
    }

    /// The selection's ranges by the path of each selected text. A measure reads them before it
    /// runs: reusing a `memo` moves its nodes out of the last commit's, leaving placeholders
    /// the selection can't be found in.
    pub(crate) fn selection_by_path(&self) -> Vec<SelectedRange> {
        self.selection_ranges().into_iter().map(|(i, a, b)| (self.nodes[i].path.clone(), a, b)).collect()
    }

    /// The line boxes of `ranges` laid out in a measure's `nodes`, found there by path.
    pub(super) fn selection_boxes_in(&self, nodes: &[Node], ranges: &[SelectedRange]) -> Vec<[f32; 4]> {
        let mut out = Vec::new();
        for (path, a, b) in ranges {
            if let Some(j) = nodes.iter().position(|n| n.path == *path) { self.text_boxes(nodes, j, *a, *b, &mut out); }
        }
        out
    }

    /// The line boxes of every `Cx::text_rects` read in `reads`, laid out in `nodes`, unscrolled.
    pub(super) fn text_reads_in(&self, nodes: &[Node], index: Option<&key_index::KeyIndex>, reads: &LayoutReads) -> Vec<(String, TextBoxes)> {
        let mut out = Vec::new();
        for read in reads {
            let Some((key, ranges)) = parse_text_read(&read.key) else { continue };
            let found = match index {
                Some(index) => index.first(nodes, key),
                None => key_index::scan_first(nodes, key),
            };
            let boxes: Vec<Vec<[f32; 4]>> = match found {
                Some(j) => {
                    let rect = self.placed(nodes, j, false);
                    ranges.iter().map(|&(a, b)| {
                        let mut line = Vec::new();
                        self.text_boxes_at(&nodes[j], rect, a, b, &mut line);
                        line
                    }).collect()
                }
                None => vec![Vec::new(); ranges.len()],
            };
            out.push((read.key.clone(), boxes.into()));
        }
        out
    }

    /// Pushes the line boxes of bytes `a..b` of node `j`'s text onto `out`.
    fn text_boxes(&self, nodes: &[Node], j: usize, a: usize, b: usize, out: &mut Vec<[f32; 4]>) {
        self.text_boxes_at(&nodes[j], self.laid_out(nodes, j), a, b, out);
    }

    /// Pushes the line boxes of bytes `a..b` of `n`'s text, laid out in `rect`, onto `out`.
    fn text_boxes_at(&self, n: &Node, rect: [f32; 4], a: usize, b: usize, out: &mut Vec<[f32; 4]>) {
        let (Some(spec), Some(layout)) = (&n.text, &n.text_layout) else { return };
        let len = spec.content.len();
        let (a, b) = (a.min(len), b.min(len));
        if a >= b { return; }
        let (a, b) = n.elided.map_or((a, b), |e| e.range(a, b));
        let [x, y, w, h] = rect;
        let s = self.env.scale;
        let [ox, oy] = links::origin(n, spec, layout, [x, y, x + w, y + h], s, 1.0);
        for (bb, _) in crate::text::range_boxes(layout, a, b, true) {
            let (x0, y0) = (ox + bb.x0 as f32 / s, oy + bb.y0 as f32 / s);
            out.push([x0, y0, (bb.x1 - bb.x0) as f32 / s, (bb.y1 - bb.y0) as f32 / s]);
        }
    }

    /// Once a frame: the selection moved since the view last read it (a drag, a click, code,
    /// scrolling). That isn't layout feedback, so the view re-runs as for a state change.
    pub(super) fn selection_read_moved(&self) -> bool {
        let seen = &self.layout_seen.rects;
        seen.contains_key(SELECTION) && boxes_moved(&seen.selection, &self.selection_boxes())
    }
}
