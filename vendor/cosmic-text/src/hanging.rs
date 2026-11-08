// SPDX-License-Identifier: MIT OR Apache-2.0
// (Deductum's patch to cosmic-text 0.19.0: see vendor/cosmic-text/README.md)

//! A hanging indent: a wrapped line continues at its own indentation plus a few columns.

/// Where a [`Buffer`](crate::Buffer)'s wrapped rows continue: at the line's own leading blanks and
/// these columns of its font further in
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HangingIndent {
    /// Columns past the line's own indentation. A column is a monospace font's advance, or
    /// otherwise the width of the line's first glyph
    pub columns: f32,
}

#[cfg(feature = "std")]
std::thread_local! {
    static FOR_NEW_BUFFERS: core::cell::Cell<Option<HangingIndent>> = const { core::cell::Cell::new(None) };
}

/// Run `make` with every [`Buffer`](crate::Buffer) it creates on this thread starting with this
/// hanging indent. For a buffer created inside another crate's widget, where
/// [`Buffer::set_hanging_indent`](crate::Buffer::set_hanging_indent) cannot reach it. The setting
/// stays with the buffer; buffers created elsewhere are not affected
#[cfg(feature = "std")]
pub fn with_hanging_indent<T>(hanging_indent: Option<HangingIndent>, make: impl FnOnce() -> T) -> T {
    struct Restore(Option<HangingIndent>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FOR_NEW_BUFFERS.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(FOR_NEW_BUFFERS.with(|cell| cell.replace(hanging_indent)));
    make()
}

/// What a new buffer starts with
pub(crate) fn for_new_buffer() -> Option<HangingIndent> {
    #[cfg(feature = "std")]
    {
        FOR_NEW_BUFFERS.with(core::cell::Cell::get)
    }
    #[cfg(not(feature = "std"))]
    {
        None
    }
}

#[cfg(feature = "std")]
std::thread_local! {
    static JUSTIFY_NEW_BUFFERS: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// Run `make` with every [`Buffer`](crate::Buffer) it creates on this thread starting justified
/// (see [`Buffer::set_justified`](crate::Buffer::set_justified)), as
/// [`with_hanging_indent`] does for the hanging indent
#[cfg(feature = "std")]
pub fn with_justified<T>(justified: bool, make: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            JUSTIFY_NEW_BUFFERS.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(JUSTIFY_NEW_BUFFERS.with(|cell| cell.replace(justified)));
    make()
}

/// Whether a new buffer starts justified
pub(crate) fn justified_for_new_buffer() -> bool {
    #[cfg(feature = "std")]
    {
        JUSTIFY_NEW_BUFFERS.with(core::cell::Cell::get)
    }
    #[cfg(not(feature = "std"))]
    {
        false
    }
}
