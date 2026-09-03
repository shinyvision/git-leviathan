//! Media widgets: animated raster playback (plugins + built-ins), and the
//! diff viewer's image canvas, transport timeline and video surface.

pub mod animated_raster;
pub mod image_viewer;
pub mod timeline;
pub mod video_surface;

pub use animated_raster::{animated_raster, load_animated_raster, AnimatedRaster};
