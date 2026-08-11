//! Render memory report: pixel dimensions, exact buffer size, and peak-RSS
//! growth for a page rendered at 72/150/300/600 DPI.
//!
//! Complements the criterion latency benches (`benches/render.rs`) with the
//! memory side of the story: how much pixel data each resolution produces
//! and how far it pushes the process's peak resident set size.
//!
//! Run against the default fixture or any PDF:
//!
//! ```sh
//! cargo run --release --example memory_report
//! cargo run --release --example memory_report -- path/to/file.pdf
//! ```

use firecrawl_pdfium::{Pdfium, RenderConfig};

/// Peak-RSS readout via `getrusage(2)`. `std` exposes no peak-RSS API, so
/// this declares the one libc symbol it needs directly; the `unsafe` is
/// confined to this module. This is example-grade plumbing, not crate API.
#[cfg(unix)]
mod peak_rss {
    use std::ffi::{c_int, c_long};

    /// `struct timeval`. On the 64-bit unixes this example targets the
    /// real struct is 16 bytes (macOS pads a 32-bit `tv_usec` up to the
    /// 8-byte alignment of `tv_sec`), so two `c_long` fields reproduce the
    /// exact size and alignment. The values are never read here.
    #[repr(C)]
    #[derive(Default)]
    struct Timeval {
        tv_sec: c_long,
        tv_usec: c_long,
    }

    /// `struct rusage` as specified by POSIX and laid out identically on
    /// macOS and Linux (glibc): two timevals followed by fourteen longs,
    /// of which `ru_maxrss` is the first.
    #[repr(C)]
    #[derive(Default)]
    struct Rusage {
        ru_utime: Timeval,
        ru_stime: Timeval,
        ru_maxrss: c_long,
        ru_ixrss: c_long,
        ru_idrss: c_long,
        ru_isrss: c_long,
        ru_minflt: c_long,
        ru_majflt: c_long,
        ru_nswap: c_long,
        ru_inblock: c_long,
        ru_oublock: c_long,
        ru_msgsnd: c_long,
        ru_msgrcv: c_long,
        ru_nsignals: c_long,
        ru_nvcsw: c_long,
        ru_nivcsw: c_long,
    }

    /// `RUSAGE_SELF`: statistics for the calling process. The value is 0
    /// on every unix.
    const RUSAGE_SELF: c_int = 0;

    extern "C" {
        fn getrusage(who: c_int, usage: *mut Rusage) -> c_int;
    }

    /// The process's peak resident set size so far, in bytes, or `None`
    /// if the call fails.
    pub fn peak_rss_bytes() -> Option<u64> {
        let mut usage = Rusage::default();
        // SAFETY: `usage` is a valid, writable value whose layout matches
        // `struct rusage` on the platforms compiled here (see above), and
        // RUSAGE_SELF is always a valid `who`.
        let rc = unsafe { getrusage(RUSAGE_SELF, &mut usage) };
        if rc != 0 {
            return None;
        }
        let raw = u64::try_from(usage.ru_maxrss).ok()?;
        // The unit of ru_maxrss is bytes on macOS but kilobytes on Linux
        // and the BSDs.
        Some(if cfg!(target_os = "macos") {
            raw
        } else {
            raw * 1024
        })
    }
}

#[cfg(not(unix))]
mod peak_rss {
    /// Peak RSS is not implemented off unix; the report prints "n/a".
    pub fn peak_rss_bytes() -> Option<u64> {
        None
    }
}

struct Row {
    dpi: u32,
    width: u32,
    height: u32,
    buffer_bytes: u64,
    rss_delta: Option<u64>,
}

fn human_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hello_text.pdf").to_string()
    });

    let pdfium = Pdfium::load()?;
    let doc = pdfium.load_document_from_file(&path, None)?;
    let page = doc.page(0)?;

    println!("document : {path}");
    println!(
        "page 0   : {:.1} x {:.1} pt ({} page{} total)",
        page.width(),
        page.height(),
        doc.page_count(),
        if doc.page_count() == 1 { "" } else { "s" },
    );
    println!("format   : Bgra8 (4 bytes/pixel)");
    println!();

    let mut rows = Vec::new();
    for dpi in [72u32, 150, 300, 600] {
        let config = RenderConfig::new().dpi(dpi as f32);
        let rss_before = peak_rss::peak_rss_bytes();
        let rendered = page.render(&config)?;
        let rss_after = peak_rss::peak_rss_bytes();

        rows.push(Row {
            dpi,
            width: rendered.width(),
            height: rendered.height(),
            buffer_bytes: rendered.pixels().len() as u64,
            rss_delta: match (rss_before, rss_after) {
                (Some(before), Some(after)) => Some(after.saturating_sub(before)),
                _ => None,
            },
        });
        // Drop each buffer before the next (larger) render so a row's
        // peak-RSS delta approximates that render's own footprint rather
        // than the sum of every buffer so far.
        drop(rendered);
    }

    println!(
        "{:>5}  {:>13}  {:>22}  {:>14}",
        "dpi", "pixels", "buffer", "peak-RSS delta"
    );
    for row in &rows {
        let buffer = format!("{} ({} B)", human_bytes(row.buffer_bytes), row.buffer_bytes);
        let delta = match row.rss_delta {
            Some(d) => human_bytes(d),
            None => "n/a".to_string(),
        };
        println!(
            "{:>5}  {:>13}  {:>22}  {:>14}",
            row.dpi,
            format!("{}x{}", row.width, row.height),
            buffer,
            delta,
        );
    }
    println!();
    println!(
        "Note: peak RSS is monotonic, so a render that fits inside an earlier
peak shows a zero delta, and allocator reuse of freed pages makes deltas
approximate rather than equal to buffer sizes."
    );

    Ok(())
}
