//! Trackpad gestures.
//!
//! A two-finger scroll already reaches the app as an iced scroll delta, so
//! panning with it is decided in the viewport (see `scroll_intent`). A pinch
//! does not: winit reports it as `WindowEvent::PinchGesture`, and iced's winit
//! layer converts only the window events it has an iced equivalent for — a
//! gesture is not one of them, so it is dropped before the app ever sees it.
//! The gesture is therefore read straight from AppKit with a local event
//! monitor and handed to the app through an iced subscription, the same shape
//! the SpaceMouse bridge uses.

/// Magnification deltas from a pinch, positive meaning zoom in.
///
/// Never fires off macOS: there is no pinch gesture to read.
pub(crate) fn subscription() -> iced::Subscription<f32> {
    #[cfg(target_os = "macos")]
    {
        appkit::subscription()
    }
    #[cfg(not(target_os = "macos"))]
    {
        iced::Subscription::none()
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    use iced::futures::{channel::mpsc, Stream};
    use std::sync::{Mutex, OnceLock};

    /// The subscription's output, parked where the event monitor — which runs
    /// on the main thread, outside iced — can push into it. `None` until the
    /// subscription is first built, and replaced if iced ever rebuilds it.
    static SINK: OnceLock<Mutex<Option<mpsc::Sender<f32>>>> = OnceLock::new();

    pub(super) fn subscription() -> iced::Subscription<f32> {
        install_monitor();
        iced::Subscription::run_with(SinkId, stream)
    }

    /// Distinguishes this subscription; the value never changes, so the stream
    /// stays alive across updates.
    #[derive(Hash)]
    struct SinkId;

    fn stream(_: &SinkId) -> impl Stream<Item = f32> + use<> {
        iced::stream::channel(64, |output| async move {
            sink().lock().unwrap().replace(output);
            // Park forever: the app holds the live channel through `SINK`, so
            // returning here would retire the subscription that feeds it.
            std::future::pending::<()>().await;
        })
    }

    fn sink() -> &'static Mutex<Option<mpsc::Sender<f32>>> {
        SINK.get_or_init(|| Mutex::new(None))
    }

    /// Hand one magnification to the app. A full channel means the app is
    /// thousands of events behind, so dropping the event is the right failure.
    fn publish(magnification: f32) {
        if let Some(slot) = SINK.get() {
            if let Some(sender) = slot.lock().unwrap().as_mut() {
                let _ = sender.try_send(magnification);
            }
        }
    }

    fn install_monitor() {
        use block2::RcBlock;
        use objc2_app_kit::{NSEvent, NSEventMask};
        use std::ptr::NonNull;
        use std::sync::Once;

        static INSTALL: Once = Once::new();
        // AppKit wants the monitor installed on the main thread, which is where
        // iced builds subscriptions. The block itself also runs there, during
        // AppKit's own dispatch, and only touches the sink — never app state.
        INSTALL.call_once(|| {
            let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit owns the event for the length of this call.
                let magnification = unsafe { event.as_ref().magnification() } as f32;
                publish(magnification);
                // Pass the event along untouched; the pinch is not consumed.
                event.as_ptr()
            });

            // The monitor token is deliberately dropped: the app has a single
            // window for its whole life, and releasing the token does not
            // uninstall the monitor anyway (`removeMonitor` does).
            unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::Magnify, &block)
            };
        });
    }
}
