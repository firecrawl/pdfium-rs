//! Coordinate spaces and the pixel↔page transform.
//!
//! Two coordinate spaces appear throughout this crate:
//!
//! - **Page space** — PDF user space: units of points (1/72 inch), origin at
//!   the *bottom-left* of the page, y increasing *upward*. Text character
//!   boxes and PDF content live here.
//! - **Pixel space** — a rendered bitmap: units of pixels, origin at the
//!   *top-left*, y increasing *downward*.
//!
//! [`PageTransform`] converts between them for one specific render
//! geometry. It is derived from PDFium's own `FPDF_DeviceToPage` mapping at
//! render time (so `/Rotate` entries and extra render rotations follow
//! PDFium's exact semantics) and is plain data afterwards: it stays valid
//! after the page and document are closed.

/// A point in page space (points, origin bottom-left, y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePoint {
    /// Horizontal position in points, from the left page edge.
    pub x: f64,
    /// Vertical position in points, from the *bottom* page edge (y-up).
    pub y: f64,
}

impl PagePoint {
    /// Creates a page-space point.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A point in pixel space (pixels, origin top-left, y-down).
///
/// Coordinates are `f64` so sub-pixel positions survive round trips; pixel
/// *indices* map to the pixel's top-left corner (the center of pixel
/// `(3, 7)` is `(3.5, 7.5)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelPoint {
    /// Horizontal position in pixels, from the left bitmap edge.
    pub x: f64,
    /// Vertical position in pixels, from the *top* bitmap edge (y-down).
    pub y: f64,
}

impl PixelPoint {
    /// Creates a pixel-space point.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle in page space.
///
/// Follows PDF conventions: `bottom <= top` and `left <= right` when
/// normalized (y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRect {
    /// Smallest x edge, in points.
    pub left: f64,
    /// Smallest y edge, in points (page space is y-up).
    pub bottom: f64,
    /// Largest x edge, in points.
    pub right: f64,
    /// Largest y edge, in points.
    pub top: f64,
}

impl PageRect {
    /// Creates a page-space rectangle from its four edges.
    pub fn new(left: f64, bottom: f64, right: f64, top: f64) -> Self {
        Self { left, bottom, right, top }
    }

    /// Horizontal extent (`right - left`).
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    /// Vertical extent (`top - bottom`).
    pub fn height(&self) -> f64 {
        self.top - self.bottom
    }

    /// Returns the same rectangle with `left <= right` and `bottom <= top`.
    pub fn normalized(&self) -> PageRect {
        PageRect {
            left: self.left.min(self.right),
            right: self.left.max(self.right),
            bottom: self.bottom.min(self.top),
            top: self.bottom.max(self.top),
        }
    }
}

/// An axis-aligned rectangle in pixel space: top-left corner plus size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    /// Left edge in pixels.
    pub x: f64,
    /// Top edge in pixels.
    pub y: f64,
    /// Width in pixels.
    pub width: f64,
    /// Height in pixels.
    pub height: f64,
}

impl PixelRect {
    /// Creates a pixel-space rectangle from top-left corner and size.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
}

/// Affine transform between pixel space of one rendered bitmap and page
/// space of the page it was rendered from.
///
/// Obtained from [`RenderedPage::transform`] or
/// [`PdfPage::transform_for`]. Plain data: freely `Clone`/`Send`/`Sync`,
/// and independent of any PDFium resource.
///
/// [`RenderedPage::transform`]: crate::RenderedPage::transform
/// [`PdfPage::transform_for`]: crate::PdfPage::transform_for
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageTransform {
    /// Pixel→page coefficients: `page = (a*x + b*y + e, c*x + d*y + f)`.
    fwd: [f64; 6],
    /// Page→pixel coefficients, same layout.
    inv: [f64; 6],
    pixel_width: u32,
    pixel_height: u32,
}

impl PageTransform {
    /// Builds a transform from PDFium's page-space images of the three
    /// device corners `(0,0)`, `(w,0)`, `(0,h)`.
    pub(crate) fn from_corners(
        pixel_width: u32,
        pixel_height: u32,
        origin: (f64, f64),  // page coords of device (0, 0)
        x_axis: (f64, f64),  // page coords of device (w, 0)
        y_axis: (f64, f64),  // page coords of device (0, h)
    ) -> Option<PageTransform> {
        let w = f64::from(pixel_width);
        let h = f64::from(pixel_height);
        let a = (x_axis.0 - origin.0) / w;
        let c = (x_axis.1 - origin.1) / w;
        let b = (y_axis.0 - origin.0) / h;
        let d = (y_axis.1 - origin.1) / h;
        let (e, f) = origin;

        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let ia = d / det;
        let ib = -b / det;
        let ic = -c / det;
        let id = a / det;
        let ie = -(ia * e + ib * f);
        let if_ = -(ic * e + id * f);

        Some(PageTransform {
            fwd: [a, b, c, d, e, f],
            inv: [ia, ib, ic, id, ie, if_],
            pixel_width,
            pixel_height,
        })
    }

    /// Width in pixels of the bitmap this transform describes.
    pub fn pixel_width(&self) -> u32 {
        self.pixel_width
    }

    /// Height in pixels of the bitmap this transform describes.
    pub fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    /// Maps a pixel-space point to page space.
    pub fn pixel_to_page(&self, p: PixelPoint) -> PagePoint {
        let [a, b, c, d, e, f] = self.fwd;
        PagePoint::new(a * p.x + b * p.y + e, c * p.x + d * p.y + f)
    }

    /// Maps a page-space point to pixel space.
    pub fn page_to_pixel(&self, p: PagePoint) -> PixelPoint {
        let [a, b, c, d, e, f] = self.inv;
        PixelPoint::new(a * p.x + b * p.y + e, c * p.x + d * p.y + f)
    }

    /// Maps a pixel-space rectangle to a normalized page-space rectangle.
    pub fn pixel_rect_to_page(&self, r: PixelRect) -> PageRect {
        let p1 = self.pixel_to_page(PixelPoint::new(r.x, r.y));
        let p2 = self.pixel_to_page(PixelPoint::new(r.x + r.width, r.y + r.height));
        PageRect::new(p1.x, p1.y, p2.x, p2.y).normalized()
    }

    /// Maps a page-space rectangle to a pixel-space rectangle
    /// (top-left + size, with non-negative size).
    pub fn page_rect_to_pixel(&self, r: PageRect) -> PixelRect {
        let p1 = self.page_to_pixel(PagePoint::new(r.left, r.top));
        let p2 = self.page_to_pixel(PagePoint::new(r.right, r.bottom));
        let x = p1.x.min(p2.x);
        let y = p1.y.min(p2.y);
        PixelRect::new(x, y, (p2.x - p1.x).abs(), (p2.y - p1.y).abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_like_roundtrip() {
        // A 200x100pt page rendered at 2x: device (0,0) -> page (0, 100),
        // device (400,0) -> page (200, 100), device (0,200) -> page (0, 0).
        let t = PageTransform::from_corners(400, 200, (0.0, 100.0), (200.0, 100.0), (0.0, 0.0))
            .unwrap();
        let p = t.pixel_to_page(PixelPoint::new(100.0, 50.0));
        assert!((p.x - 50.0).abs() < 1e-9);
        assert!((p.y - 75.0).abs() < 1e-9);
        let d = t.page_to_pixel(p);
        assert!((d.x - 100.0).abs() < 1e-9);
        assert!((d.y - 50.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_corners_rejected() {
        assert!(
            PageTransform::from_corners(100, 100, (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)).is_none()
        );
    }
}
