//! Render wrapped text as canvas `Path` fills.
//!
//! General-purpose cosmic-text glyph-outline tessellation: lays out a string
//! into a wrapped buffer and converts each glyph's outline into a filled
//! `canvas::Path`. Independent of the toast card it currently serves.
use iced::advanced::graphics::text::cosmic_text::Command as ZenoCommand;
use iced::advanced::{
    graphics::text::{self as graphics_text, cosmic_text},
    text::{Shaping, Wrapping},
};
use iced::{widget::canvas, Color, Point};

#[derive(Debug, Clone)]
pub(super) struct CachedPathFill {
    pub(super) path: canvas::Path,
    pub(super) color: Color,
}

pub(super) fn measure_wrapped_text_height(
    content: &str,
    max_width: f32,
    font_size: f32,
    line_height: f32,
) -> f32 {
    with_wrapped_buffer(
        content,
        max_width,
        font_size,
        line_height,
        |buffer: &cosmic_text::Buffer,
         _: &mut graphics_text::FontSystem,
         _: &mut cosmic_text::SwashCache| {
            graphics_text::measure(buffer).0.height.max(line_height)
        },
    )
}

pub(super) fn push_wrapped_text_paths(
    content: &str,
    origin: Point,
    max_width: f32,
    font_size: f32,
    line_height: f32,
    color: Color,
    fills: &mut Vec<CachedPathFill>,
) -> f32 {
    with_wrapped_buffer(
        content,
        max_width,
        font_size,
        line_height,
        |buffer: &cosmic_text::Buffer,
         font_system: &mut graphics_text::FontSystem,
         swash_cache: &mut cosmic_text::SwashCache| {
            for run in buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    let offset = Point::new(
                        origin.x + glyph.x + glyph.x_offset,
                        origin.y + run.line_y + glyph.y_offset,
                    );
                    let cache_key = glyph.physical((0.0, 0.0), 1.0).cache_key;

                    if let Some(commands) =
                        swash_cache.get_outline_commands(font_system.raw(), cache_key)
                    {
                        let path = canvas::Path::new(|path| {
                            for command in commands {
                                match command {
                                    ZenoCommand::MoveTo(point) => {
                                        path.move_to(Point::new(
                                            point.x + offset.x,
                                            -point.y + offset.y,
                                        ));
                                    }
                                    ZenoCommand::LineTo(point) => {
                                        path.line_to(Point::new(
                                            point.x + offset.x,
                                            -point.y + offset.y,
                                        ));
                                    }
                                    ZenoCommand::CurveTo(control_a, control_b, to) => {
                                        path.bezier_curve_to(
                                            Point::new(
                                                control_a.x + offset.x,
                                                -control_a.y + offset.y,
                                            ),
                                            Point::new(
                                                control_b.x + offset.x,
                                                -control_b.y + offset.y,
                                            ),
                                            Point::new(to.x + offset.x, -to.y + offset.y),
                                        );
                                    }
                                    ZenoCommand::QuadTo(control, to) => {
                                        path.quadratic_curve_to(
                                            Point::new(control.x + offset.x, -control.y + offset.y),
                                            Point::new(to.x + offset.x, -to.y + offset.y),
                                        );
                                    }
                                    ZenoCommand::Close => {
                                        path.close();
                                    }
                                }
                            }
                        });

                        fills.push(CachedPathFill { path, color });
                    }
                }
            }

            graphics_text::measure(buffer).0.height.max(line_height)
        },
    )
}

fn with_wrapped_buffer<R>(
    content: &str,
    max_width: f32,
    font_size: f32,
    line_height: f32,
    f: impl FnOnce(
        &cosmic_text::Buffer,
        &mut graphics_text::FontSystem,
        &mut cosmic_text::SwashCache,
    ) -> R,
) -> R {
    let mut font_system = graphics_text::font_system()
        .write()
        .expect("write font system");
    let metrics = cosmic_text::Metrics::new(font_size, line_height.max(f32::MIN_POSITIVE));
    let mut buffer = cosmic_text::Buffer::new(font_system.raw(), metrics);

    buffer.set_wrap(
        font_system.raw(),
        graphics_text::to_wrap(Wrapping::WordOrGlyph),
    );
    buffer.set_size(font_system.raw(), Some(max_width), None);
    let attrs = graphics_text::to_attributes(iced::Font::default());
    buffer.set_text(
        font_system.raw(),
        content,
        &attrs,
        graphics_text::to_shaping(Shaping::Advanced, content),
        None,
    );

    let mut swash_cache = cosmic_text::SwashCache::new();

    f(&buffer, &mut font_system, &mut swash_cache)
}
