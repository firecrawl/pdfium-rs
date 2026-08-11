//! Rendering: configuration, execution, and owned results.

use std::ffi::c_int;

use crate::coords::PageTransform;
use crate::error::{Error, Result};
use crate::page::{PdfPage, Rotation};
use crate::sys;

/// Pixel layout of a rendered bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PixelFormat {
    /// 4 bytes/pixel: blue, green, red, alpha — straight (non-premultiplied)
    /// alpha. PDFium's native format.
    #[default]
    Bgra8,
    /// 4 bytes/pixel: red, green, blue, alpha — straight alpha. Rendered by
    /// PDFium with its reverse-byte-order flag; convenient for `image`/PNG
    /// interop.
    Rgba8,
    /// 3 bytes/pixel: blue, green, red. No alpha.
    Bgr8,
    /// 1 byte/pixel grayscale.
    Gray8,
}

impl PixelFormat {
    /// Bytes per pixel.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => 4,
            PixelFormat::Bgr8 => 3,
            PixelFormat::Gray8 => 1,
        }
    }

    /// Whether the format carries an alpha channel.
    pub fn has_alpha(self) -> bool {
        matches!(self, PixelFormat::Bgra8 | PixelFormat::Rgba8)
    }

    fn as_fpdf(self) -> c_int {
        match self {
            // Rgba8 is BGRA storage rendered with FPDF_REVERSE_BYTE_ORDER.
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => sys::FPDFBitmap_BGRA,
            PixelFormat::Bgr8 => sys::FPDFBitmap_BGR,
            PixelFormat::Gray8 => sys::FPDFBitmap_Gray,
        }
    }
}

/// An sRGB color with straight alpha, used for render backgrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha (255 = opaque), straight (non-premultiplied).
    pub a: u8,
}

impl Color {
    /// Opaque white — the default render background.
    pub const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);
    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0x00, 0x00, 0x00);
    /// Fully transparent — use with an alpha [`PixelFormat`] for
    /// compositing.
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// An opaque color from RGB components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 0xFF }
    }

    /// A color from RGBA components (straight alpha).
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// Rec. 601 luma, used when encoding into grayscale buffers.
    fn luma(self) -> u8 {
        let y = 0.299 * f32::from(self.r) + 0.587 * f32::from(self.g) + 0.114 * f32::from(self.b);
        y.round().clamp(0.0, 255.0) as u8
    }

    /// Encodes into the in-memory byte pattern for one pixel of `format`.
    fn encode(self, format: PixelFormat) -> ([u8; 4], usize) {
        match format {
            PixelFormat::Bgra8 => ([self.b, self.g, self.r, self.a], 4),
            PixelFormat::Rgba8 => ([self.r, self.g, self.b, self.a], 4),
            PixelFormat::Bgr8 => ([self.b, self.g, self.r, 0], 3),
            PixelFormat::Gray8 => ([self.luma(), 0, 0, 0], 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SizeSpec {
    /// Multiply page points by this factor.
    Scale(f32),
    /// Fixed output width in pixels; height keeps the aspect ratio.
    Width(u32),
    /// Fixed output height in pixels; width keeps the aspect ratio.
    Height(u32),
    /// Largest size that fits within the box while keeping aspect ratio.
    Fit(u32, u32),
    /// Exact output dimensions; aspect ratio may change.
    Exact(u32, u32),
}

/// Configuration for [`PdfPage::render`].
///
/// ```
/// use firecrawl_pdfium::{RenderConfig, PixelFormat};
///
/// // 300 DPI grayscale, annotations off:
/// let config = RenderConfig::new()
///     .dpi(300.0)
///     .pixel_format(PixelFormat::Gray8)
///     .annotations(false);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RenderConfig {
    size: SizeSpec,
    format: PixelFormat,
    background: Color,
    annotations: bool,
    form_fields: bool,
    extra_rotation: Rotation,
    text_antialiasing: bool,
    image_antialiasing: bool,
    path_antialiasing: bool,
    max_output_bytes: u64,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            size: SizeSpec::Scale(1.0),
            format: PixelFormat::Bgra8,
            background: Color::WHITE,
            annotations: true,
            form_fields: true,
            extra_rotation: Rotation::None,
            text_antialiasing: true,
            image_antialiasing: true,
            path_antialiasing: true,
            max_output_bytes: RenderConfig::DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

impl RenderConfig {
    /// Default output-size ceiling: 1 GiB of pixel data.
    pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1 << 30;

    /// A configuration rendering at 1:1 (72 DPI), BGRA, white background,
    /// annotations and form fields on, 1 GiB output cap.
    pub fn new() -> RenderConfig {
        RenderConfig::default()
    }

    /// Scale factor over page points (2.0 renders a 612×792pt page at
    /// 1224×1584px). Mutually exclusive with the other size selectors;
    /// the last one set wins.
    pub fn scale(mut self, factor: f32) -> Self {
        self.size = SizeSpec::Scale(factor);
        self
    }

    /// Resolution in dots per inch (72 DPI == scale 1.0).
    pub fn dpi(mut self, dpi: f32) -> Self {
        self.size = SizeSpec::Scale(dpi / 72.0);
        self
    }

    /// Fixed output width in pixels, preserving aspect ratio.
    pub fn width(mut self, pixels: u32) -> Self {
        self.size = SizeSpec::Width(pixels);
        self
    }

    /// Fixed output height in pixels, preserving aspect ratio.
    pub fn height(mut self, pixels: u32) -> Self {
        self.size = SizeSpec::Height(pixels);
        self
    }

    /// Largest output that fits in `width`×`height`, preserving aspect
    /// ratio.
    pub fn fit(mut self, width: u32, height: u32) -> Self {
        self.size = SizeSpec::Fit(width, height);
        self
    }

    /// Exact output dimensions (may distort the aspect ratio).
    pub fn exact(mut self, width: u32, height: u32) -> Self {
        self.size = SizeSpec::Exact(width, height);
        self
    }

    /// Output pixel format (default [`PixelFormat::Bgra8`]).
    pub fn pixel_format(mut self, format: PixelFormat) -> Self {
        self.format = format;
        self
    }

    /// Background color the page is composited over (default white). Use
    /// [`Color::TRANSPARENT`] with an alpha format for compositing.
    pub fn background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// Render annotations (default `true`).
    pub fn annotations(mut self, on: bool) -> Self {
        self.annotations = on;
        self
    }

    /// Draw AcroForm field appearances (default `true`). Takes effect only
    /// when the document has
    /// [`enable_form_rendering`](crate::PdfDocument::enable_form_rendering)
    /// active.
    pub fn form_fields(mut self, on: bool) -> Self {
        self.form_fields = on;
        self
    }

    /// Extra rotation applied at render time, on top of the page's own
    /// `/Rotate` (default none). 90°/270° swap the output dimensions.
    pub fn rotate(mut self, rotation: Rotation) -> Self {
        self.extra_rotation = rotation;
        self
    }

    /// Toggle text anti-aliasing (default `true`).
    pub fn text_antialiasing(mut self, on: bool) -> Self {
        self.text_antialiasing = on;
        self
    }

    /// Toggle image anti-aliasing (default `true`).
    pub fn image_antialiasing(mut self, on: bool) -> Self {
        self.image_antialiasing = on;
        self
    }

    /// Toggle path anti-aliasing (default `true`).
    pub fn path_antialiasing(mut self, on: bool) -> Self {
        self.path_antialiasing = on;
        self
    }

    /// Ceiling on the pixel buffer size in bytes
    /// (default [`RenderConfig::DEFAULT_MAX_OUTPUT_BYTES`], 1 GiB).
    /// Renders that would exceed it fail with [`Error::RenderTooLarge`]
    /// *before* allocating. Use `u64::MAX` to disable.
    pub fn max_output_bytes(mut self, limit: u64) -> Self {
        self.max_output_bytes = limit;
        self
    }

    /// The output dimensions this configuration produces for a page of the
    /// given size (points, post-`/Rotate`).
    pub(crate) fn resolve_dimensions(&self, page: crate::PageSize) -> Result<(u32, u32)> {
        // Work in device orientation: extra 90°/270° rotation swaps axes.
        let (pw, ph) = if self.extra_rotation.swaps_axes() {
            (page.height as f64, page.width as f64)
        } else {
            (page.width as f64, page.height as f64)
        };
        if !(pw.is_finite() && ph.is_finite()) || pw <= 0.0 || ph <= 0.0 {
            return Err(Error::InvalidConfig(format!(
                "page has degenerate dimensions {pw}x{ph}pt"
            )));
        }

        let scaled = |scale: f64| -> Result<(u32, u32)> {
            if !scale.is_finite() || scale <= 0.0 {
                return Err(Error::InvalidConfig(format!(
                    "scale must be positive, got {scale}"
                )));
            }
            Ok((
                (pw * scale).round().max(1.0) as u32,
                (ph * scale).round().max(1.0) as u32,
            ))
        };

        let (w, h) = match self.size {
            SizeSpec::Scale(s) => scaled(f64::from(s))?,
            SizeSpec::Width(px) => {
                nonzero(px, "width")?;
                scaled(f64::from(px) / pw)?
            }
            SizeSpec::Height(px) => {
                nonzero(px, "height")?;
                scaled(f64::from(px) / ph)?
            }
            SizeSpec::Fit(bw, bh) => {
                nonzero(bw, "fit width")?;
                nonzero(bh, "fit height")?;
                scaled((f64::from(bw) / pw).min(f64::from(bh) / ph))?
            }
            SizeSpec::Exact(w, h) => {
                nonzero(w, "width")?;
                nonzero(h, "height")?;
                (w, h)
            }
        };

        // PDFium's bitmap API takes i32 dimensions and strides. The byte
        // requirement is computed in u128: the saturating f64->u32 casts
        // above can leave w and h at u32::MAX, whose product times bpp
        // overflows u64.
        let bpp = u128::from(self.format.bytes_per_pixel() as u64);
        let required = u128::from(w) * u128::from(h) * bpp;
        let required_bytes = u64::try_from(required).unwrap_or(u64::MAX);
        let too_large = w > i32::MAX as u32
            || h > i32::MAX as u32
            || u128::from(w) * bpp > i32::MAX as u128
            || required > u128::from(self.max_output_bytes);
        if too_large {
            return Err(Error::RenderTooLarge {
                required_bytes,
                limit: self.max_output_bytes,
            });
        }
        Ok((w, h))
    }

    fn flags(&self) -> c_int {
        let mut flags = 0;
        if self.annotations {
            flags |= sys::FPDF_ANNOT;
        }
        if self.format == PixelFormat::Rgba8 {
            flags |= sys::FPDF_REVERSE_BYTE_ORDER;
        }
        if !self.text_antialiasing {
            flags |= sys::FPDF_RENDER_NO_SMOOTHTEXT;
        }
        if !self.image_antialiasing {
            flags |= sys::FPDF_RENDER_NO_SMOOTHIMAGE;
        }
        if !self.path_antialiasing {
            flags |= sys::FPDF_RENDER_NO_SMOOTHPATH;
        }
        flags
    }
}

fn nonzero(v: u32, what: &str) -> Result<()> {
    if v == 0 {
        return Err(Error::InvalidConfig(format!("{what} must be nonzero")));
    }
    Ok(())
}

/// A rendered page: owned pixels plus everything needed to interpret them.
///
/// Contains **no PDFium resources** — it is plain data, freely `Send +
/// Sync`, and remains valid after the page, document, and even the library
/// handle are gone.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    width: u32,
    height: u32,
    stride: usize,
    format: PixelFormat,
    data: Vec<u8>,
    page_index: usize,
    transform: PageTransform,
}

impl RenderedPage {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Bytes per row. This crate always produces dense buffers
    /// (`stride == width * bytes_per_pixel`), but consume [`stride`] rather
    /// than assuming density.
    ///
    /// [`stride`]: Self::stride
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Pixel format of [`pixels`](Self::pixels).
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// The raw pixel data, `height * stride` bytes, rows top to bottom.
    pub fn pixels(&self) -> &[u8] {
        &self.data
    }

    /// Consumes the render, returning the pixel buffer.
    pub fn into_pixels(self) -> Vec<u8> {
        self.data
    }

    /// One row of pixels.
    ///
    /// # Panics
    ///
    /// Panics if `y >= height`.
    pub fn row(&self, y: u32) -> &[u8] {
        assert!(
            y < self.height,
            "row {y} out of bounds (height {})",
            self.height
        );
        let start = y as usize * self.stride;
        &self.data[start..start + self.width as usize * self.format.bytes_per_pixel()]
    }

    /// The bytes of one pixel.
    ///
    /// # Panics
    ///
    /// Panics if out of bounds.
    pub fn pixel(&self, x: u32, y: u32) -> &[u8] {
        assert!(
            x < self.width,
            "column {x} out of bounds (width {})",
            self.width
        );
        let bpp = self.format.bytes_per_pixel();
        let row = self.row(y);
        &row[x as usize * bpp..(x as usize + 1) * bpp]
    }

    /// 0-based index of the page this was rendered from.
    pub fn page_index(&self) -> usize {
        self.page_index
    }

    /// The pixel↔page-space transform for exactly this render geometry.
    pub fn transform(&self) -> &PageTransform {
        &self.transform
    }

    /// Converts to tightly packed RGBA8 (e.g. for `image::RgbaImage` or PNG
    /// encoders), whatever the source format.
    pub fn to_rgba8(&self) -> Vec<u8> {
        let w = self.width as usize;
        let h = self.height as usize;
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let row = &self.data[y * self.stride..];
            match self.format {
                PixelFormat::Rgba8 => out.extend_from_slice(&row[..w * 4]),
                PixelFormat::Bgra8 => {
                    for px in row[..w * 4].chunks_exact(4) {
                        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                }
                PixelFormat::Bgr8 => {
                    for px in row[..w * 3].chunks_exact(3) {
                        out.extend_from_slice(&[px[2], px[1], px[0], 0xFF]);
                    }
                }
                PixelFormat::Gray8 => {
                    for &g in &row[..w] {
                        out.extend_from_slice(&[g, g, g, 0xFF]);
                    }
                }
            }
        }
        out
    }
}

impl<'doc> PdfPage<'doc> {
    /// Renders this page to an owned pixel buffer.
    ///
    /// The whole page is rendered (no partial viewports in this version);
    /// resolution, format, background, rotation, and layer toggles come
    /// from `config`.
    pub fn render(&self, config: &RenderConfig) -> Result<RenderedPage> {
        let (width, height) = config.resolve_dimensions(self.size())?;
        let bpp = config.format.bytes_per_pixel();
        let stride = width as usize * bpp;
        let mut data = vec![0u8; stride * height as usize];
        fill_background(&mut data, config.format, config.background);

        let rotate = config.extra_rotation.as_raw();
        let flags = config.flags();
        let draw_forms = config.form_fields && self.document().form_env().is_some();

        let transform = self.ffi(|b| -> Result<PageTransform> {
            // SAFETY: dimensions/stride validated i32-safe by
            // resolve_dimensions; `data` outlives the bitmap handle (both
            // live in this frame, bitmap destroyed below); external-buffer
            // mode means FPDFBitmap_Destroy will not free `data`.
            let bitmap = unsafe {
                b.FPDFBitmap_CreateEx(
                    width as c_int,
                    height as c_int,
                    config.format.as_fpdf(),
                    data.as_mut_ptr().cast(),
                    stride as c_int,
                )
            };
            if bitmap.is_null() {
                return Err(Error::RenderFailed {
                    reason: "FPDFBitmap_CreateEx returned null",
                });
            }

            // SAFETY: live bitmap + page handles; geometry matches the
            // bitmap dimensions exactly.
            unsafe {
                b.FPDF_RenderPageBitmap(
                    bitmap,
                    self.handle(),
                    0,
                    0,
                    width as c_int,
                    height as c_int,
                    rotate,
                    flags,
                );
            }

            if draw_forms {
                if let Some(env) = self.document().form_env() {
                    // SAFETY: live handles; header: call FPDF_FFLDraw
                    // "after rendering functions ... have finished
                    // rendering the page contents", same geometry.
                    unsafe {
                        b.FPDF_FFLDraw(
                            env.handle(),
                            bitmap,
                            self.handle(),
                            0,
                            0,
                            width as c_int,
                            height as c_int,
                            rotate,
                            flags,
                        );
                    }
                }
            }

            // SAFETY: live bitmap handle, destroyed exactly once. The pixel
            // buffer is ours and survives.
            unsafe { b.FPDFBitmap_Destroy(bitmap) };

            derive_transform(b, self.handle(), width, height, rotate)
        })?;

        Ok(RenderedPage {
            width,
            height,
            stride,
            format: config.format,
            data,
            page_index: self.index(),
            transform,
        })
    }

    /// Computes the pixel↔page transform for `config` **without
    /// rendering** — useful to plan layouts or map coordinates for a render
    /// that will happen elsewhere.
    pub fn transform_for(&self, config: &RenderConfig) -> Result<PageTransform> {
        let (width, height) = config.resolve_dimensions(self.size())?;
        let rotate = config.extra_rotation.as_raw();
        self.ffi(|b| derive_transform(b, self.handle(), width, height, rotate))
    }

    /// PDFium's own device→page conversion (`FPDF_DeviceToPage`) for the
    /// geometry `config` would produce.
    ///
    /// PDFium's C API takes *integer* device coordinates, so `pixel` is
    /// rounded to the nearest pixel first. [`PageTransform`] (from
    /// [`transform_for`](Self::transform_for) or a render) offers the same
    /// mapping in continuous coordinates without an FFI call; the two agree
    /// to within the integer quantization (tested).
    pub fn device_to_page(
        &self,
        config: &RenderConfig,
        pixel: crate::PixelPoint,
    ) -> Result<crate::PagePoint> {
        let (width, height) = config.resolve_dimensions(self.size())?;
        let rotate = config.extra_rotation.as_raw();
        let dx = clamp_to_c_int(pixel.x)?;
        let dy = clamp_to_c_int(pixel.y)?;
        let (mut px, mut py) = (0.0f64, 0.0f64);
        // SAFETY: live page handle, valid out-pointers; viewport parameters
        // match what a render with this config would use, as PDFium
        // requires.
        let ok = self.ffi(|b| unsafe {
            b.FPDF_DeviceToPage(
                self.handle(),
                0,
                0,
                width as c_int,
                height as c_int,
                rotate,
                dx,
                dy,
                &mut px,
                &mut py,
            )
        });
        if ok != 0 {
            Ok(crate::PagePoint::new(px, py))
        } else {
            Err(Error::RenderFailed {
                reason: "FPDF_DeviceToPage failed",
            })
        }
    }

    /// PDFium's own page→device conversion (`FPDF_PageToDevice`) for the
    /// geometry `config` would produce.
    ///
    /// PDFium returns *integer* device coordinates, so the result is
    /// quantized to whole pixels. Use [`PageTransform`] for sub-pixel
    /// precision; the two agree to within one pixel (tested).
    pub fn page_to_device(
        &self,
        config: &RenderConfig,
        point: crate::PagePoint,
    ) -> Result<crate::PixelPoint> {
        let (width, height) = config.resolve_dimensions(self.size())?;
        let rotate = config.extra_rotation.as_raw();
        let (mut dx, mut dy) = (0 as c_int, 0 as c_int);
        // SAFETY: live page handle, valid out-pointers; viewport parameters
        // match what a render with this config would use.
        let ok = self.ffi(|b| unsafe {
            b.FPDF_PageToDevice(
                self.handle(),
                0,
                0,
                width as c_int,
                height as c_int,
                rotate,
                point.x,
                point.y,
                &mut dx,
                &mut dy,
            )
        });
        if ok != 0 {
            Ok(crate::PixelPoint::new(f64::from(dx), f64::from(dy)))
        } else {
            Err(Error::RenderFailed {
                reason: "FPDF_PageToDevice failed",
            })
        }
    }
}

fn clamp_to_c_int(v: f64) -> Result<c_int> {
    let r = v.round();
    if r.is_finite() && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&r) {
        Ok(r as c_int)
    } else {
        Err(Error::InvalidConfig(format!(
            "device coordinate {v} is outside the addressable integer range"
        )))
    }
}

/// Derives the affine pixel↔page transform by asking PDFium for the page
/// coordinates of three exact integer device corners, using the same
/// viewport parameters as the render (a documented requirement of
/// `FPDF_DeviceToPage`).
fn derive_transform(
    b: &sys::Bindings,
    page: sys::FPDF_PAGE,
    width: u32,
    height: u32,
    rotate: c_int,
) -> Result<PageTransform> {
    let corner = |dx: c_int, dy: c_int| -> Result<(f64, f64)> {
        let (mut px, mut py) = (0.0f64, 0.0f64);
        // SAFETY: live page handle, valid out-pointers, geometry matches
        // the associated render call.
        let ok = unsafe {
            b.FPDF_DeviceToPage(
                page,
                0,
                0,
                width as c_int,
                height as c_int,
                rotate,
                dx,
                dy,
                &mut px,
                &mut py,
            )
        };
        if ok != 0 {
            Ok((px, py))
        } else {
            Err(Error::RenderFailed {
                reason: "FPDF_DeviceToPage failed",
            })
        }
    };

    let origin = corner(0, 0)?;
    let x_axis = corner(width as c_int, 0)?;
    let y_axis = corner(0, height as c_int)?;
    PageTransform::from_corners(width, height, origin, x_axis, y_axis).ok_or(Error::RenderFailed {
        reason: "degenerate page transform",
    })
}

/// Pre-fills the buffer with the background color encoded for `format`.
/// (PDFium composites page content over existing buffer contents; its own
/// FillRect *replaces* pixels rather than compositing, so filling ourselves
/// is both simpler and exact for every format including reversed byte
/// order.)
fn fill_background(data: &mut [u8], format: PixelFormat, color: Color) {
    let (pattern, bpp) = color.encode(format);
    let pattern = &pattern[..bpp];
    if pattern.iter().all(|&b| b == pattern[0]) {
        data.fill(pattern[0]);
    } else {
        for px in data.chunks_exact_mut(bpp) {
            px.copy_from_slice(pattern);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::PageSize;

    fn dims(cfg: &RenderConfig, w: f32, h: f32) -> Result<(u32, u32)> {
        cfg.resolve_dimensions(PageSize {
            width: w,
            height: h,
        })
    }

    #[test]
    fn scale_and_dpi() {
        assert_eq!(
            dims(&RenderConfig::new().scale(2.0), 200.0, 100.0).unwrap(),
            (400, 200)
        );
        assert_eq!(
            dims(&RenderConfig::new().dpi(144.0), 200.0, 100.0).unwrap(),
            (400, 200)
        );
    }

    #[test]
    fn fixed_axes_preserve_aspect() {
        assert_eq!(
            dims(&RenderConfig::new().width(400), 200.0, 100.0).unwrap(),
            (400, 200)
        );
        assert_eq!(
            dims(&RenderConfig::new().height(50), 200.0, 100.0).unwrap(),
            (100, 50)
        );
        assert_eq!(
            dims(&RenderConfig::new().fit(1000, 300), 200.0, 100.0).unwrap(),
            (600, 300)
        );
        assert_eq!(
            dims(&RenderConfig::new().exact(37, 91), 200.0, 100.0).unwrap(),
            (37, 91)
        );
    }

    #[test]
    fn rotation_swaps_output_axes() {
        let cfg = RenderConfig::new().scale(1.0).rotate(Rotation::Clockwise90);
        assert_eq!(dims(&cfg, 200.0, 100.0).unwrap(), (100, 200));
        // width() means *output* width, post-rotation.
        let cfg = RenderConfig::new().width(300).rotate(Rotation::Clockwise90);
        assert_eq!(dims(&cfg, 200.0, 100.0).unwrap(), (300, 600));
    }

    #[test]
    fn size_cap_enforced() {
        let cfg = RenderConfig::new().scale(100.0).max_output_bytes(1024);
        match dims(&cfg, 200.0, 100.0) {
            Err(Error::RenderTooLarge {
                required_bytes,
                limit,
            }) => {
                assert_eq!(limit, 1024);
                assert!(required_bytes > 1024);
            }
            other => panic!("expected RenderTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn invalid_inputs_rejected() {
        assert!(matches!(
            dims(&RenderConfig::new().scale(0.0), 200.0, 100.0),
            Err(Error::InvalidConfig(_))
        ));
        assert!(matches!(
            dims(&RenderConfig::new().scale(f32::NAN), 200.0, 100.0),
            Err(Error::InvalidConfig(_))
        ));
        assert!(matches!(
            dims(&RenderConfig::new().exact(0, 10), 200.0, 100.0),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn background_fill_patterns() {
        let mut buf = vec![0u8; 12];
        fill_background(&mut buf, PixelFormat::Bgra8, Color::rgba(1, 2, 3, 4));
        assert_eq!(&buf[..4], &[3, 2, 1, 4]);
        fill_background(&mut buf, PixelFormat::Rgba8, Color::rgba(1, 2, 3, 4));
        assert_eq!(&buf[..4], &[1, 2, 3, 4]);
        let mut buf3 = vec![0u8; 9];
        fill_background(&mut buf3, PixelFormat::Bgr8, Color::rgb(10, 20, 30));
        assert_eq!(&buf3[..3], &[30, 20, 10]);
    }
}
