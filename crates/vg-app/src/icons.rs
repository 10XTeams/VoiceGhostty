//! 线条图标,直接用画笔画。不用 emoji:各平台字体不一定有(之前 🧩 ⇧ ✕ 都显示成方块),
//! 风格也对不齐。所有图标画在 16×16 的网格上,线宽统一,随尺寸缩放。

use std::f32::consts::{PI, TAU};

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{vec2, Color32, Painter, Pos2, Rect, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Close,
    Gear,
    Keyboard,
    Blocks,
    Sparkle,
    Search,
    ChevronUp,
    ChevronDown,
    ChevronRight,
    Folder,
    Trash,
    ArrowUp,
    Refresh,
    Mic,
    Check,
    Alert,
    Play,
    Expand,
    Collapse,
}

struct Pen<'a> {
    painter: &'a Painter,
    origin: Pos2,
    unit: f32,
    stroke: Stroke,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.unit
    }
    fn line(&self, pts: &[(f32, f32)]) {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(PathShape::line(points, PathStroke::from(self.stroke)));
    }
    fn closed(&self, pts: &[(f32, f32)]) {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(PathShape::closed_line(points, PathStroke::from(self.stroke)));
    }
    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_stroke(self.p(x, y), r * self.unit, self.stroke);
    }
    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_filled(self.p(x, y), r * self.unit, self.stroke.color);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let rect = Rect::from_min_size(self.p(x, y), vec2(w, h) * self.unit);
        self.painter.rect_stroke(rect, r * self.unit, self.stroke, egui::StrokeKind::Middle);
    }
    fn arc(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.line(&pts);
    }
    fn curve(&self, a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) {
        let shape = CubicBezierShape::from_points_stroke(
            [self.p(a.0, a.1), self.p(b.0, b.1), self.p(c.0, c.1), self.p(d.0, d.1)],
            false,
            Color32::TRANSPARENT,
            self.stroke,
        );
        self.painter.add(shape);
    }
}

/// 在 rect 中心画一个边长 size 的图标
pub fn paint(painter: &Painter, rect: Rect, size: f32, icon: Icon, color: Color32) {
    let unit = size / 16.0;
    let origin = rect.center() - vec2(8.0, 8.0) * unit;
    let width = (1.35 * unit).max(1.1);
    let pen = Pen { painter, origin, unit, stroke: Stroke::new(width, color) };
    match icon {
        Icon::Plus => {
            pen.line(&[(8.0, 3.0), (8.0, 13.0)]);
            pen.line(&[(3.0, 8.0), (13.0, 8.0)]);
        }
        Icon::Close => {
            pen.line(&[(4.0, 4.0), (12.0, 12.0)]);
            pen.line(&[(12.0, 4.0), (4.0, 12.0)]);
        }
        Icon::Gear => {
            pen.circle(8.0, 8.0, 2.2);
            // 齿轮外廓:8 个齿,内外半径交替
            let mut pts = vec![];
            let teeth = 8;
            for i in 0..teeth * 4 {
                let a = i as f32 / (teeth * 4) as f32 * TAU - PI / 2.0;
                let r = if (i % 4) == 1 || (i % 4) == 2 { 6.4 } else { 5.0 };
                pts.push((8.0 + r * a.cos(), 8.0 + r * a.sin()));
            }
            pen.closed(&pts);
        }
        Icon::Keyboard => {
            pen.rect(1.5, 3.5, 13.0, 9.0, 1.8);
            for x in [4.25, 6.75, 9.25, 11.75] {
                pen.dot(x, 6.3, 0.62);
            }
            for x in [4.25, 11.75] {
                pen.dot(x, 8.6, 0.62);
            }
            pen.line(&[(6.0, 9.9), (10.0, 9.9)]);
        }
        Icon::Blocks => {
            pen.rect(2.0, 2.0, 5.0, 5.0, 1.2);
            pen.rect(2.0, 9.0, 5.0, 5.0, 1.2);
            pen.rect(9.0, 9.0, 5.0, 5.0, 1.2);
            pen.line(&[(11.5, 1.8), (11.5, 7.2)]);
            pen.line(&[(8.8, 4.5), (14.2, 4.5)]);
        }
        Icon::Sparkle => {
            let star = |cx: f32, cy: f32, r: f32| {
                let k = r * 0.22;
                pen.closed(&[
                    (cx, cy - r),
                    (cx + k, cy - k),
                    (cx + r, cy),
                    (cx + k, cy + k),
                    (cx, cy + r),
                    (cx - k, cy + k),
                    (cx - r, cy),
                    (cx - k, cy - k),
                ]);
            };
            star(7.0, 9.0, 5.6);
            star(12.6, 3.6, 2.4);
        }
        Icon::Search => {
            pen.circle(7.0, 7.0, 4.3);
            pen.line(&[(10.2, 10.2), (13.6, 13.6)]);
        }
        Icon::ChevronUp => pen.line(&[(4.0, 10.0), (8.0, 6.0), (12.0, 10.0)]),
        Icon::ChevronDown => pen.line(&[(4.0, 6.0), (8.0, 10.0), (12.0, 6.0)]),
        Icon::ChevronRight => pen.line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)]),
        Icon::Folder => pen.closed(&[
            (1.8, 4.0),
            (6.2, 4.0),
            (7.6, 5.6),
            (14.2, 5.6),
            (14.2, 12.6),
            (1.8, 12.6),
        ]),
        Icon::Trash => {
            pen.line(&[(2.5, 4.5), (13.5, 4.5)]);
            pen.line(&[(6.0, 4.5), (6.5, 2.5), (9.5, 2.5), (10.0, 4.5)]);
            pen.line(&[(4.0, 4.5), (4.8, 13.5), (11.2, 13.5), (12.0, 4.5)]);
            pen.line(&[(6.8, 7.0), (7.0, 11.0)]);
            pen.line(&[(9.2, 7.0), (9.0, 11.0)]);
        }
        Icon::ArrowUp => {
            pen.line(&[(8.0, 13.0), (8.0, 3.5)]);
            pen.line(&[(4.0, 7.5), (8.0, 3.5), (12.0, 7.5)]);
        }
        Icon::Refresh => {
            pen.arc(8.0, 8.0, 5.0, -PI * 0.35, PI * 1.45);
            let tip = (8.0 + 5.0 * (-PI * 0.35_f32).cos(), 8.0 + 5.0 * (-PI * 0.35_f32).sin());
            pen.line(&[(tip.0 - 3.0, tip.1 - 0.6), tip, (tip.0 - 0.4, tip.1 + 3.0)]);
        }
        Icon::Mic => {
            pen.rect(5.5, 1.5, 5.0, 8.0, 2.5);
            pen.curve((3.2, 7.6), (3.2, 13.0), (12.8, 13.0), (12.8, 7.6));
            pen.line(&[(8.0, 11.7), (8.0, 14.4)]);
        }
        Icon::Check => pen.line(&[(3.2, 8.4), (6.6, 11.6), (12.8, 4.6)]),
        Icon::Alert => {
            pen.closed(&[(8.0, 2.0), (14.4, 13.4), (1.6, 13.4)]);
            pen.line(&[(8.0, 6.3), (8.0, 9.4)]);
            pen.dot(8.0, 11.4, 0.8);
        }
        Icon::Play => {
            let points = vec![pen.p(5.0, 3.5), pen.p(12.5, 8.0), pen.p(5.0, 12.5)];
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Expand => {
            pen.line(&[(4.0, 3.5), (8.0, 7.0), (12.0, 3.5)]);
            pen.line(&[(4.0, 9.0), (8.0, 12.5), (12.0, 9.0)]);
        }
        Icon::Collapse => {
            pen.line(&[(4.0, 7.0), (8.0, 3.5), (12.0, 7.0)]);
            pen.line(&[(4.0, 12.5), (8.0, 9.0), (12.0, 12.5)]);
        }
    }
}
