use iced::{
    mouse,
    widget::canvas::{self, path::Arc, Canvas, Frame, Geometry, Path, Program, Stroke},
    Color, Element, Length, Point, Radians, Rectangle, Renderer, Theme,
};
use std::time::Instant;

const SPIN_PERIOD_SECS: f32 = 0.9;
const ARC_SWEEP_FRAC: f32 = 0.75;
const STROKE_WIDTH: f32 = 1.6;

pub struct SpinnerProgram {
    started_at: Instant,
    color: Color,
}

impl<Message> Program<Message> for SpinnerProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        use std::f32::consts::TAU;

        let mut frame = Frame::new(renderer, bounds.size());
        let cx = bounds.width / 2.0;
        let cy = bounds.height / 2.0;
        let radius = (bounds.width.min(bounds.height) / 2.0) - STROKE_WIDTH;
        if radius <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let elapsed = self.started_at.elapsed().as_secs_f32();
        let start = (elapsed / SPIN_PERIOD_SECS) * TAU;
        let end = start + TAU * ARC_SWEEP_FRAC;

        let path = Path::new(|b| {
            b.arc(Arc {
                center: Point::new(cx, cy),
                radius,
                start_angle: Radians(start),
                end_angle: Radians(end),
            });
        });

        frame.stroke(
            &path,
            Stroke::default()
                .with_color(self.color)
                .with_width(STROKE_WIDTH)
                .with_line_cap(canvas::LineCap::Round),
        );

        vec![frame.into_geometry()]
    }
}

pub fn spinner<'a, Message: 'a>(
    started_at: Instant,
    color: Color,
    size: f32,
) -> Element<'a, Message> {
    Canvas::new(SpinnerProgram { started_at, color })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}
