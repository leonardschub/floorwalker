//! The Wayland subsurface the world presents into (spec: "Attachment"; ADR-0009; the
//! spike's `WaylandSub`).
//!
//! Bevy cannot present on the GTK toplevel's own `wl_surface`: GTK commits its own
//! buffers there, and two producers on one surface fight. Instead the shell opens GTK's
//! `wl_display` from Rust, creates a new `wl_surface`, and attaches it as a
//! `wl_subsurface` of the toplevel placed *below* it. The compositor shows it wherever
//! the transparent webview paints nothing. All of this runs on the main (GTK) thread.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use wayland_client::{
    backend::{Backend, ObjectId},
    globals::registry_queue_init,
    protocol::{
        wl_compositor::WlCompositor, wl_subcompositor::WlSubcompositor,
        wl_subsurface::WlSubsurface, wl_surface::WlSurface,
    },
    Connection, EventQueue, Proxy,
};

mod dispatch {
    use wayland_client::{
        delegate_noop,
        globals::GlobalListContents,
        protocol::{
            wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
            wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
        },
        Connection, Dispatch, QueueHandle,
    };

    /// The shell's own event queue state. None of these objects sends events it needs.
    pub struct State;

    impl Dispatch<WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: wayland_client::protocol::wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    delegate_noop!(State: WlCompositor);
    delegate_noop!(State: WlSubcompositor);
    delegate_noop!(State: WlSubsurface);
    delegate_noop!(State: WlRegion);
    delegate_noop!(State: ignore WlSurface);
}

/// The world's surface: a `wl_subsurface` below the main window's toplevel surface.
pub struct Subsurface {
    conn: Connection,
    queue: EventQueue<dispatch::State>,
    surface: WlSurface,
    subsurface: WlSubsurface,
    display: NonNull<c_void>,
    position: (i32, i32),
    scale: i32,
}

// Safety: wayland-client proxies are Send + Sync. The subsurface is created, moved and
// read only on the main thread; the event queue is touched only there.
unsafe impl Send for Subsurface {}

impl Subsurface {
    /// Attaches a new subsurface below `toplevel` (a `wl_surface*` owned by GTK) on
    /// `display` (GTK's `wl_display*`).
    pub fn attach(display: NonNull<c_void>, toplevel: NonNull<c_void>) -> Result<Self, String> {
        let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) =
            registry_queue_init::<dispatch::State>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = queue.handle();
        let compositor: WlCompositor = globals
            .bind(&qh, 4..=6, ())
            .map_err(|e| format!("wl_compositor: {e}"))?;
        let subcompositor: WlSubcompositor = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wl_subcompositor: {e}"))?;
        let parent_id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), toplevel.as_ptr().cast()) }
                .map_err(|e| format!("toplevel surface id: {e:?}"))?;
        let parent =
            WlSurface::from_id(&conn, parent_id).map_err(|e| format!("toplevel surface: {e:?}"))?;

        let surface = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
        subsurface.place_below(&parent);
        subsurface.set_desync();
        // Input belongs to the webview: the subsurface takes none.
        let empty = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&empty));
        empty.destroy();
        surface.commit();
        queue
            .roundtrip(&mut dispatch::State)
            .map_err(|e| format!("roundtrip: {e}"))?;
        Ok(Self {
            conn,
            queue,
            surface,
            subsurface,
            display,
            position: (0, 0),
            scale: 1,
        })
    }

    /// Moves the subsurface to the webview's position in the toplevel (logical px) and
    /// sets its buffer scale to GTK's.
    pub fn place(&mut self, x: i32, y: i32, scale: i32) {
        if (x, y) != self.position {
            self.subsurface.set_position(x, y);
            self.position = (x, y);
        }
        if scale != self.scale {
            self.surface.set_buffer_scale(scale);
            self.scale = scale;
        }
        let _ = self.queue.dispatch_pending(&mut dispatch::State);
        let _ = self.conn.flush();
    }

    /// Destroys the subsurface and its surface. Call only once nothing presents into the
    /// surface any more: after the world thread is joined.
    pub fn destroy(self) {
        self.subsurface.destroy();
        self.surface.destroy();
        let _ = self.conn.flush();
    }

    /// A clock on this surface's frame callbacks, for the world thread.
    pub fn frames(&self) -> FrameClock {
        FrameClock {
            conn: self.conn.clone(),
            queue: self.conn.new_event_queue(),
            surface: self.surface.clone(),
            state: frames::State::default(),
            asked: VecDeque::new(),
        }
    }

    /// The raw handles Bevy presents through.
    pub fn handles(&self) -> SurfaceHandles {
        let surface = NonNull::new(self.surface.id().as_ptr().cast()).expect("live wl_surface");
        SurfaceHandles {
            window: RawWindowHandle::Wayland(WaylandWindowHandle::new(surface)),
            display: RawDisplayHandle::Wayland(WaylandDisplayHandle::new(self.display)),
        }
    }
}

/// Raw Wayland handles to the subsurface, handed to Bevy's renderer.
#[derive(Clone, Copy)]
pub struct SurfaceHandles {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

// Safety: these are Wayland object pointers; libwayland and Mesa's WSI are thread-safe,
// and the subsurface they point to outlives the world that presents into it.
unsafe impl Send for SurfaceHandles {}
unsafe impl Sync for SurfaceHandles {}

impl HasWindowHandle for SurfaceHandles {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}

impl HasDisplayHandle for SurfaceHandles {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

/// The webview widget's position inside the toplevel and its size (logical px), with
/// GTK's integer scale factor.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: i32,
}

impl Geometry {
    pub fn of(webview: &webkit2gtk::WebView) -> Self {
        use gtk::prelude::WidgetExt;
        let (x, y) = webview
            .toplevel()
            .and_then(|top| webview.translate_coordinates(&top, 0, 0))
            .unwrap_or((0, 0));
        Self {
            x,
            y,
            width: webview.allocated_width(),
            height: webview.allocated_height(),
            scale: webview.scale_factor().max(1),
        }
    }

    /// The drawable size in physical pixels.
    pub fn physical(&self) -> (u32, u32) {
        (
            (self.width.max(1) * self.scale) as u32,
            (self.height.max(1) * self.scale) as u32,
        )
    }
}

mod frames {
    use wayland_client::{
        protocol::wl_callback::{self, WlCallback},
        Connection, Dispatch, QueueHandle,
    };

    /// The frame clock's own queue state: how many of the frame callbacks it asked for
    /// have come since it last looked.
    #[derive(Default)]
    pub struct State {
        pub done: usize,
    }

    impl Dispatch<WlCallback, ()> for State {
        fn event(
            state: &mut Self,
            _: &WlCallback,
            event: wl_callback::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_callback::Event::Done { .. } = event {
                state.done += 1;
            }
        }
    }
}

/// How many drawn frames may await their frame callback at once. With one, the world
/// waits for each frame to be shown before it starts the next, so the CPU's part of a
/// frame never overlaps the GPU's and a frame that takes a little longer than a refresh
/// costs three (ticket 27). With more, the next frame is worked out while the last one
/// is still being drawn or waits to be shown. A frame's callback comes only once the
/// compositor has shown it, a refresh or two after the GPU finished it, so with two a
/// GPU-bound world still waited on the compositor now and then; three let paced frames
/// come as fast as unpaced ones in the nested KWin. How far the GPU itself may fall
/// behind is wgpu's own frame latency (two frames).
pub const IN_FLIGHT: usize = 3;

/// The world's suspension mechanism (spike problem 1) and its pace: the subsurface's own
/// frame callbacks tell the world thread when the compositor shows what it drew.
///
/// The world asks for a frame callback with every frame it draws (the request rides on
/// Mesa's commit of that frame) and draws while fewer than [`IN_FLIGHT`] frames await
/// theirs. KWin sends none to a minimised window or one on another virtual desktop: the
/// world then draws at most [`IN_FLIGHT`] more frames and keeps simulating without
/// drawing; when the window shows again, KWin sends the callbacks and drawing resumes by
/// itself. The world presents with `Mailbox` (`engine`), so no present ever waits for a
/// callback in Mesa and a frame that is not shown yet is simply replaced by a newer one.
///
/// The clock only dispatches its own queue. GTK's main loop reads the display socket and
/// routes the callback's event here; reading the socket from this thread as well would
/// wait on GTK's own prepared read.
pub struct FrameClock {
    conn: Connection,
    queue: EventQueue<frames::State>,
    surface: WlSurface,
    state: frames::State,
    /// When each outstanding frame callback was asked for, oldest first. The compositor
    /// answers them in order.
    asked: VecDeque<Instant>,
}

// Safety: the proxies are Send + Sync, and the queue is used only by the world thread
// once it has been handed over.
unsafe impl Send for FrameClock {}

impl FrameClock {
    /// Asks for a frame callback with the next commit: call right before drawing a frame.
    pub fn ask(&mut self) {
        let qh = self.queue.handle();
        self.surface.frame(&qh, ());
        self.asked.push_back(Instant::now());
        let _ = self.conn.flush();
    }

    /// Waits until a frame may be drawn, or until `deadline`: until no drawn frame awaits
    /// its callback, or (from `early` on) fewer than [`IN_FLIGHT`] do. True when a frame
    /// may be drawn now without running ahead of the compositor.
    pub fn wait(&mut self, early: Instant, deadline: Instant, in_flight: usize) -> bool {
        loop {
            let waiting = self.outstanding();
            let now = Instant::now();
            if waiting == 0 || (waiting < in_flight && now >= early) {
                return true;
            }
            if now >= deadline {
                return false;
            }
            std::thread::sleep((deadline - now).min(Duration::from_millis(1)));
        }
    }

    /// How long the oldest outstanding frame callback has been awaited, if one is.
    pub fn awaited(&self) -> Option<Duration> {
        self.asked.front().map(|asked| asked.elapsed())
    }

    /// How many drawn frames still await their frame callback.
    fn outstanding(&mut self) -> usize {
        if self.asked.is_empty() {
            return 0;
        }
        let _ = self.queue.dispatch_pending(&mut self.state);
        for _ in 0..std::mem::take(&mut self.state.done) {
            self.asked.pop_front();
        }
        self.asked.len()
    }
}
