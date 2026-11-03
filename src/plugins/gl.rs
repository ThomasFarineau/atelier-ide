//! The OpenGL functions of the window, by name, for the paint callbacks of native plugins: a
//! plugin loads its own `glow` from them. eframe does not give its loader away, so this one asks
//! the system as glutin does: WGL on Windows, EGL then GLX on Linux, the OpenGL framework on
//! macOS.

use std::{
    ffi::{CStr, c_char, c_void},
    sync::LazyLock,
};

type GetProcAddress = unsafe extern "system" fn(*const c_char) -> *const c_void;

struct Loader {
    /// The library that exports the functions of OpenGL 1.1 (and the rest on macOS).
    library: Option<libloading::Library>,
    /// The loader of the extensions and of what came after OpenGL 1.1.
    get: Option<GetProcAddress>,
}

fn open(names: &[&str]) -> Option<libloading::Library> {
    // SAFETY: system libraries, already loaded by eframe in practice.
    names
        .iter()
        .find_map(|name| unsafe { libloading::Library::new(name) }.ok())
}

fn symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Option<T> {
    // SAFETY: the type matches the documented signature of each symbol asked for.
    unsafe { library.get::<T>(name).ok().map(|s| *s) }
}

static LOADER: LazyLock<Loader> = LazyLock::new(|| {
    #[cfg(windows)]
    {
        let library = open(&["opengl32.dll"]);
        let get = library
            .as_ref()
            .and_then(|l| symbol::<GetProcAddress>(l, b"wglGetProcAddress\0"));
        Loader { library, get }
    }
    #[cfg(target_os = "macos")]
    {
        Loader {
            library: open(&["/System/Library/Frameworks/OpenGL.framework/OpenGL"]),
            get: None,
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        type Current = unsafe extern "C" fn() -> *const c_void;
        // EGL when its context is the current one, else GLX.
        if let Some(egl) = open(&["libEGL.so.1", "libEGL.so"])
            && let Some(current) = symbol::<Current>(&egl, b"eglGetCurrentContext\0")
            // SAFETY: a query with no argument.
            && !unsafe { current() }.is_null()
        {
            let get = symbol::<GetProcAddress>(&egl, b"eglGetProcAddress\0");
            return Loader {
                library: Some(egl),
                get,
            };
        }
        let library = open(&["libGL.so.1", "libGL.so"]);
        let get = library
            .as_ref()
            .and_then(|l| symbol::<GetProcAddress>(l, b"glXGetProcAddressARB\0"));
        Loader { library, get }
    }
});

/// The address of an OpenGL function of the current context, null when unknown.
///
/// # Safety
/// `name` is a C string.
pub unsafe extern "C" fn get_proc_address(name: *const c_char) -> *const c_void {
    let loader = &*LOADER;
    if let Some(get) = loader.get {
        // SAFETY: the caller's.
        let address = unsafe { get(name) };
        // wglGetProcAddress answers 1, 2, 3 or -1 for some functions it does not load.
        if !matches!(address as isize, -1..=3) {
            return address;
        }
    }
    // SAFETY: the caller's.
    let name = unsafe { CStr::from_ptr(name) };
    loader
        .library
        .as_ref()
        .and_then(|l| symbol::<unsafe extern "system" fn()>(l, name.to_bytes_with_nul()))
        .map_or(std::ptr::null(), |f| f as *const c_void)
}
