//! `Cx::text_rects` gives per-line boxes for many ranges of an editable field.
use std::cell::RefCell;
use std::ops::Range;

use rune::{Cx, El, Env, Runtime, ScrollSpec, TextEdit, TextStyle, col, text_input};

const BODY: &str = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";

struct S {
    edit: TextEdit,
    ranges: Vec<Range<usize>>,
    seen: RefCell<Vec<Vec<[f32; 4]>>>,
}

fn view(s: &S, cx: &mut Cx<S>) -> El {
    *s.seen.borrow_mut() = cx.text_rects("doc", &s.ranges);
    let field = text_input(&s.edit, TextStyle::new(14.0, 400.0).lh(20.0)).on_edit(|s: &mut S| &mut s.edit).key("doc").w_full();
    col().w(200.0).h(120.0).scroll(ScrollSpec { stick_bottom: false }).child(col().w_full().h(600.0).child(field))
}

fn settle(rt: &mut Runtime<S>) {
    for _ in 0..600 {
        if !rt.frame(1.0 / 60.0) {
            return;
        }
    }
    panic!("never settled");
}

fn runtime(ranges: Vec<Range<usize>>) -> Runtime<S> {
    let s = S { edit: TextEdit::new(BODY), ranges, seen: RefCell::default() };
    let mut rt = Runtime::new(s, view, (300.0, 300.0), Env::new(true, 1.0));
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn range(word: &str) -> Range<usize> {
    let a = BODY.find(word).unwrap();
    a..a + word.len()
}

#[test]
fn a_wrapped_range_gets_one_box_per_line() {
    let rt = runtime(vec![range("one"), 0..BODY.len()]);
    let seen = rt.state.seen.borrow();
    assert_eq!(seen[0].len(), 1, "a word is on one line");
    assert!(seen[1].len() >= 3, "the whole text wraps: {:?}", seen[1]);
    let ys: Vec<f32> = seen[1].iter().map(|b| b[1]).collect();
    assert!(ys.windows(2).all(|w| w[1] > w[0]), "boxes in line order: {ys:?}");
}

#[test]
fn boxes_are_right_after_an_edit_shifts_the_ranges() {
    let mut rt = runtime(vec![range("twenty")]);
    let before = rt.state.seen.borrow()[0].clone();
    rt.update(|s, _| {
        s.edit.set_selection(0, 0);
        s.edit.insert("a good many extra words at the front ");
        s.ranges[0] = s.ranges[0].start + 38..s.ranges[0].end + 38;
    });
    settle(&mut rt);
    let after = rt.state.seen.borrow()[0].clone();
    assert_eq!(after.len(), 1);
    assert_ne!(before, after, "the box moved with its word");
}

#[test]
fn scrolling_leaves_boxes_alone() {
    let mut rt = runtime(vec![range("twenty")]);
    let before = rt.state.seen.borrow().clone();
    rt.wheel(50.0, 50.0, -40.0);
    settle(&mut rt);
    assert_eq!(before, *rt.state.seen.borrow());
}

#[test]
fn ranges_past_the_end_are_clamped_and_empty_ranges_have_no_boxes() {
    let rt = runtime(vec![BODY.len() - 3..BODY.len() + 50, 5..5]);
    let seen = rt.state.seen.borrow();
    assert_eq!(seen[0].len(), 1);
    assert!(seen[1].is_empty());
}

#[test]
fn a_missing_key_gives_empty_lists() {
    struct E;
    fn none(_: &E, cx: &mut Cx<E>) -> El {
        assert_eq!(cx.text_rects("nothing", &[0..4, 2..3]).len(), 2);
        col()
    }
    let mut rt = Runtime::new(E, none, (100.0, 100.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.frame(1.0 / 60.0);
}

#[test]
fn a_point_on_a_word_finds_its_offset_after_scrolling() {
    let mut rt = runtime(vec![range("twenty")]);
    let [x, y, w, h] = rt.state.seen.borrow()[0][0];
    let (mid_x, mid_y) = (x + w / 2.0, y + h / 2.0);
    let at = rt.element_at_in("doc", mid_x, mid_y).and_then(|p| p.text_index()).expect("text under the word");
    assert!(range("twenty").contains(&at), "{at} is inside the word");
    rt.wheel(50.0, 50.0, -40.0);
    settle(&mut rt);
    let at = rt.element_at_in("doc", mid_x, mid_y - 40.0).and_then(|p| p.text_index()).expect("text after scrolling");
    assert!(range("twenty").contains(&at), "{at} is inside the word after scrolling");
}

#[test]
fn undo_depth_counts_steps_and_follows_undo() {
    let mut e = TextEdit::new("");
    assert_eq!(e.undo_depth(), 0);
    e.insert("a");
    e.insert("b");
    assert_eq!(e.undo_depth(), 2);
    e.undo();
    assert_eq!(e.undo_depth(), 1);
    e.redo();
    assert_eq!(e.undo_depth(), 2);
}
