//! Axis-aligned geometry: rectangles, orthogonal segments, port selection and
//! the automatic router shared by every diagram type.

use crate::spec::{Point, Side};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn cx(&self) -> f64 {
        self.x + self.w / 2.0
    }

    pub fn cy(&self) -> f64 {
        self.y + self.h / 2.0
    }

    pub fn center(&self) -> Point {
        [self.cx(), self.cy()]
    }

    pub fn expanded(&self, pad: f64) -> Rect {
        Rect::new(
            self.x - pad,
            self.y - pad,
            self.w + 2.0 * pad,
            self.h + 2.0 * pad,
        )
    }

    /// Strict interior overlap; rectangles that merely touch do not intersect.
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    pub fn contains(&self, p: Point) -> bool {
        p[0] >= self.x && p[0] <= self.right() && p[1] >= self.y && p[1] <= self.bottom()
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        let r = self.right().max(o.right());
        let b = self.bottom().max(o.bottom());
        Rect::new(x, y, r - x, b - y)
    }

    pub fn from_center(c: Point, w: f64, h: f64) -> Rect {
        Rect::new(c[0] - w / 2.0, c[1] - h / 2.0, w, h)
    }

    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()
    }

    /// Distance between the rectangle and a point; zero when inside.
    pub fn distance_to_point(&self, p: Point) -> f64 {
        let dx = (self.x - p[0]).max(0.0).max(p[0] - self.right());
        let dy = (self.y - p[1]).max(0.0).max(p[1] - self.bottom());
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
}

impl Segment {
    pub fn new(a: Point, b: Point) -> Segment {
        Segment { a, b }
    }

    pub fn is_horizontal(&self) -> bool {
        (self.a[1] - self.b[1]).abs() < 1e-6
    }

    pub fn is_vertical(&self) -> bool {
        (self.a[0] - self.b[0]).abs() < 1e-6
    }

    pub fn is_orthogonal(&self) -> bool {
        self.is_horizontal() || self.is_vertical()
    }

    pub fn length(&self) -> f64 {
        ((self.a[0] - self.b[0]).powi(2) + (self.a[1] - self.b[1]).powi(2)).sqrt()
    }

    pub fn midpoint(&self) -> Point {
        [(self.a[0] + self.b[0]) / 2.0, (self.a[1] + self.b[1]) / 2.0]
    }

    /// Bounding box of the segment, with zero width or height for axis-aligned runs.
    pub fn bounds(&self) -> Rect {
        let x = self.a[0].min(self.b[0]);
        let y = self.a[1].min(self.b[1]);
        Rect::new(
            x,
            y,
            (self.a[0] - self.b[0]).abs(),
            (self.a[1] - self.b[1]).abs(),
        )
    }

    /// True when an axis-aligned segment passes through the interior of the
    /// rectangle (touching the border does not count).
    pub fn crosses_rect(&self, r: &Rect) -> bool {
        const EPS: f64 = 1e-6;
        if self.is_horizontal() {
            let y = self.a[1];
            let (x0, x1) = (self.a[0].min(self.b[0]), self.a[0].max(self.b[0]));
            y > r.y + EPS && y < r.bottom() - EPS && x0 < r.right() - EPS && x1 > r.x + EPS
        } else if self.is_vertical() {
            let x = self.a[0];
            let (y0, y1) = (self.a[1].min(self.b[1]), self.a[1].max(self.b[1]));
            x > r.x + EPS && x < r.right() - EPS && y0 < r.bottom() - EPS && y1 > r.y + EPS
        } else {
            // Diagonal fallback: sample the segment.
            (0..=16).any(|i| {
                let t = i as f64 / 16.0;
                let p = [
                    self.a[0] + (self.b[0] - self.a[0]) * t,
                    self.a[1] + (self.b[1] - self.a[1]) * t,
                ];
                r.expanded(-EPS).contains(p)
            })
        }
    }

    /// Minimum clear distance between an axis-aligned segment and a rectangle.
    pub fn distance_to_rect(&self, r: &Rect) -> f64 {
        if self.crosses_rect(r) {
            return 0.0;
        }
        if self.is_horizontal() {
            let y = self.a[1];
            let (x0, x1) = (self.a[0].min(self.b[0]), self.a[0].max(self.b[0]));
            let dx = (r.x - x1).max(0.0).max(x0 - r.right());
            let dy = (r.y - y).max(0.0).max(y - r.bottom());
            (dx * dx + dy * dy).sqrt()
        } else if self.is_vertical() {
            let x = self.a[0];
            let (y0, y1) = (self.a[1].min(self.b[1]), self.a[1].max(self.b[1]));
            let dx = (r.x - x).max(0.0).max(x - r.right());
            let dy = (r.y - y1).max(0.0).max(y0 - r.bottom());
            (dx * dx + dy * dy).sqrt()
        } else {
            r.distance_to_point(self.a).min(r.distance_to_point(self.b))
        }
    }
}

/// A point on the given side of a rectangle; `frac` walks along the side
/// (0.5 is the middle) so several ports can share one side without overlap.
pub fn port(r: &Rect, side: Side, frac: f64) -> Point {
    match side {
        Side::Left => [r.x, r.y + r.h * frac],
        Side::Right => [r.right(), r.y + r.h * frac],
        Side::Top => [r.x + r.w * frac, r.y],
        Side::Bottom => [r.x + r.w * frac, r.bottom()],
    }
}

/// Choose endpoint sides from the relative placement of two rectangles.
pub fn infer_sides(from: &Rect, to: &Rect) -> (Side, Side) {
    let horizontal_gap = to.x >= from.right() || to.right() <= from.x;
    let vertical_gap = to.y >= from.bottom() || to.bottom() <= from.y;
    let dx = to.cx() - from.cx();
    let dy = to.cy() - from.cy();
    let prefer_horizontal = if horizontal_gap && vertical_gap {
        dx.abs() >= dy.abs()
    } else {
        horizontal_gap
    };
    if prefer_horizontal {
        if dx >= 0.0 {
            (Side::Right, Side::Left)
        } else {
            (Side::Left, Side::Right)
        }
    } else if dy >= 0.0 {
        (Side::Bottom, Side::Top)
    } else {
        (Side::Top, Side::Bottom)
    }
}

/// Build an orthogonal polyline between two ports. Without waypoints the
/// route is straight, an L (one bend) or a Z (two bends) depending on the
/// side contract; explicit waypoints are honored and orthogonalized.
pub fn route(
    start: Point,
    from_side: Side,
    end: Point,
    to_side: Side,
    via: &[Point],
) -> Vec<Point> {
    let mut pts: Vec<Point> = vec![start];
    if !via.is_empty() {
        for &v in via {
            let last = *pts.last().unwrap();
            if (last[0] - v[0]).abs() > 1e-6 && (last[1] - v[1]).abs() > 1e-6 {
                // Insert a corner so the polyline stays axis-aligned; leave the
                // source side first.
                if from_side.is_horizontal() || pts.len() > 1 {
                    pts.push([v[0], last[1]]);
                } else {
                    pts.push([last[0], v[1]]);
                }
            }
            pts.push(v);
        }
        let last = *pts.last().unwrap();
        if (last[0] - end[0]).abs() > 1e-6 && (last[1] - end[1]).abs() > 1e-6 {
            if to_side.is_horizontal() {
                pts.push([last[0], end[1]]);
            } else {
                pts.push([end[0], last[1]]);
            }
        }
        pts.push(end);
        return dedup(pts);
    }
    let same_x = (start[0] - end[0]).abs() < 1e-6;
    let same_y = (start[1] - end[1]).abs() < 1e-6;
    match (from_side.is_horizontal(), to_side.is_horizontal()) {
        (true, true) => {
            if !same_y {
                let mid_x = (start[0] + end[0]) / 2.0;
                pts.push([mid_x, start[1]]);
                pts.push([mid_x, end[1]]);
            }
        }
        (false, false) => {
            if !same_x {
                let mid_y = (start[1] + end[1]) / 2.0;
                pts.push([start[0], mid_y]);
                pts.push([end[0], mid_y]);
            }
        }
        (true, false) => pts.push([end[0], start[1]]),
        (false, true) => pts.push([start[0], end[1]]),
    }
    pts.push(end);
    dedup(pts)
}

fn dedup(pts: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(pts.len());
    for p in pts {
        if let Some(last) = out.last() {
            if (last[0] - p[0]).abs() < 1e-6 && (last[1] - p[1]).abs() < 1e-6 {
                continue;
            }
        }
        out.push(p);
    }
    // Remove collinear middle points.
    let mut i = 1;
    while i + 1 < out.len() {
        let (a, b, c) = (out[i - 1], out[i], out[i + 1]);
        let collinear_h = (a[1] - b[1]).abs() < 1e-6 && (b[1] - c[1]).abs() < 1e-6;
        let collinear_v = (a[0] - b[0]).abs() < 1e-6 && (b[0] - c[0]).abs() < 1e-6;
        if collinear_h || collinear_v {
            out.remove(i);
        } else {
            i += 1;
        }
    }
    out
}

pub fn segments(poly: &[Point]) -> Vec<Segment> {
    poly.windows(2).map(|w| Segment::new(w[0], w[1])).collect()
}

/// Index of the longest segment; labels default to it.
pub fn longest_segment(poly: &[Point]) -> usize {
    let segs = segments(poly);
    let mut best = 0;
    let mut best_len = -1.0;
    for (i, s) in segs.iter().enumerate() {
        let l = s.length();
        if l > best_len + 1e-6 {
            best_len = l;
            best = i;
        }
    }
    best
}

/// Approximate rendered width of a text run in the viewer's monospace-leaning
/// UI font. Calibrated so an 11px label of 16 characters measures ~90px.
pub fn text_width(text: &str, font_px: f64) -> f64 {
    let n = text.chars().count() as f64;
    (n * font_px * 0.52).round() + 4.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_touching_do_not_intersect() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);
        assert!(!a.intersects(&b));
        assert!(a.intersects(&Rect::new(9.0, 9.0, 5.0, 5.0)));
    }

    #[test]
    fn horizontal_segment_crossing() {
        let r = Rect::new(100.0, 100.0, 50.0, 50.0);
        assert!(Segment::new([0.0, 125.0], [200.0, 125.0]).crosses_rect(&r));
        assert!(!Segment::new([0.0, 100.0], [200.0, 100.0]).crosses_rect(&r));
        assert!(!Segment::new([0.0, 50.0], [200.0, 50.0]).crosses_rect(&r));
        assert!(
            (Segment::new([0.0, 50.0], [200.0, 50.0]).distance_to_rect(&r) - 50.0).abs() < 1e-9
        );
    }

    #[test]
    fn route_is_orthogonal_and_minimal() {
        let from = Rect::new(0.0, 0.0, 100.0, 50.0);
        let to = Rect::new(300.0, 200.0, 100.0, 50.0);
        let (fs, ts) = infer_sides(&from, &to);
        assert_eq!((fs, ts), (Side::Right, Side::Left));
        let poly = route(port(&from, fs, 0.5), fs, port(&to, ts, 0.5), ts, &[]);
        assert_eq!(poly.len(), 4, "Z route has two bends");
        for s in segments(&poly) {
            assert!(s.is_orthogonal());
        }
        let straight = route([0.0, 10.0], Side::Right, [50.0, 10.0], Side::Left, &[]);
        assert_eq!(straight.len(), 2);
        let l = route([0.0, 10.0], Side::Right, [50.0, 60.0], Side::Top, &[]);
        assert_eq!(l, vec![[0.0, 10.0], [50.0, 10.0], [50.0, 60.0]]);
    }

    #[test]
    fn via_points_are_orthogonalized() {
        let poly = route(
            [0.0, 0.0],
            Side::Right,
            [100.0, 100.0],
            Side::Top,
            &[[50.0, 30.0]],
        );
        for s in segments(&poly) {
            assert!(s.is_orthogonal(), "{poly:?}");
        }
        assert_eq!(*poly.last().unwrap(), [100.0, 100.0]);
    }

    #[test]
    fn infers_vertical_when_stacked() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(20.0, 200.0, 100.0, 50.0);
        assert_eq!(infer_sides(&a, &b), (Side::Bottom, Side::Top));
        assert_eq!(infer_sides(&b, &a), (Side::Top, Side::Bottom));
    }
}
