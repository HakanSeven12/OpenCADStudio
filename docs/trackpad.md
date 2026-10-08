# Trackpad

The macOS desktop app reads trackpad gestures directly: two fingers pan, a pinch
zooms. The web build and the Linux desktop build keep zooming on every scroll
delta and have no pinch; the reasons are in Implementation below.

A mouse is unaffected. The wheel zooms, the middle button pans, and Shift
together with the middle button orbits, exactly as before.

## Navigation

| Gesture | Behavior |
| --- | --- |
| Two-finger scroll | Pan the active view, both axes, the drawing following the fingers. |
| Pinch | Zoom about the cursor, at the scale of the fingers' own spread. |
| Wheel (mouse) | Zoom about the cursor. `ZOOMFACTOR` scales a notch; `ZOOMWHEEL` reverses it. |
| Middle-button drag | Pan. Shift together with the middle button orbits the model view. |
| `PAN` / `P` | Pan with the left button until Escape. |

`ZOOMFACTOR` scales what one notch does. The system variable takes 3 to 100 —
the range it has everywhere — and `SETVAR ZOOMFACTOR` accepts nothing outside
it. Options → User Preferences reaches 500 for anyone who wants a faster wheel:
the slider covers the variable's own range, the field beside it takes any value
up to that ceiling, and both show what a notch does as a percentage. A value
above 100 therefore lives only in this application's own configuration, and a
bare `SETVAR ZOOMFACTOR` still reports it.

A notch multiplies the camera distance by `0.9 ^ (ZOOMFACTOR / 60)`, so the
default 60 brings the view a tenth closer, a notch in and a notch out cancel
exactly, and no setting can drive the distance to zero or through it. A floating
viewport in MSPACE zooms its own view the same way, from a base of 0.85 — the
15% a notch has always moved a viewport.

Panning covers the same ground whichever way it starts: the active model tile,
a paper sheet, or the model view inside a floating viewport in MSPACE. A pan
dragged out of a two-finger gesture and one dragged with the middle button move
through the same code, so they agree on speed, on the tile they pan by, and on
keeping an open selection box attached to the drawing underneath.

A pinch is not affected by `ZOOMWHEEL`. The direction of a pinch is the
direction of the fingers, not a convention about a device.

On macOS a mouse that scrolls smoothly, such as a Magic Mouse, reports precise
pixel deltas like a trackpad does, so it pans instead of zooming. That follows
from the split described below; it is not a separate setting.

## Implementation

- `src/input/trackpad.rs`: the pinch. iced's winit layer converts only the
  window events that have an iced equivalent, so winit's `PinchGesture` is
  dropped before the app sees it. The gesture is read from AppKit with a local
  `NSEvent` monitor instead, installed on the main thread where iced builds
  subscriptions. The monitor's block runs on the main thread during AppKit's own
  dispatch and only publishes the magnification into a sink; the app receives it
  through an iced subscription shaped like the SpaceMouse bridge
  (`src/input/spacemouse/`). The monitor token is never released, which does not
  uninstall the monitor — `removeMonitor` does, and the app has one window for
  its whole life.
- `src/app/update/viewport.rs`: `scroll_intent` decides what a scroll delta
  means. A wheel reports notches and zooms; a precise-scrolling device reports
  pixels and pans. There is no modifier and no setting in that decision.
  `pan_active_view` and `zoom_view_at_cursor` are the bodies shared by the mouse
  and the gestures. A pinch of `m` leaves the view exactly `1 + m` times closer:
  `pinch_zoom_steps` asks the camera for the step that applies that ratio
  (`Camera::zoom_steps_for_ratio`) instead of inverting the zoom law here, which
  is what let the two drift apart when the law stopped being a subtraction.
- `src/app/mod.rs` carries one `Message::TrackpadPinch(f32)` per gesture event.

Pixel deltas pan only where they can only mean a trackpad. The web build
receives every wheel notch as a pixel delta from the browser, so there — and on
Linux, where a smooth-scrolling mouse reports them the same way — they keep
zooming. A pinch exists nowhere else either: winit reports `PinchGesture` on
macOS and iOS only, its `PanGesture` is iOS-only, and its Wayland backend does
not bind the pointer-gesture protocol at all.

## Validation

```sh
cargo test --locked --lib scroll_intent
```

The tests cover the decision between a notch and a pixel, and the 1:1 mapping of
a pinch onto the camera — the mapping by zooming a real `Camera` and measuring
the distance it ends at, so it is checked against the camera's own zoom law and
not against a second copy of it. The gestures themselves are checked by hand: macOS
offers no way to synthesize a magnify event, so a pinch cannot be driven by a
test. Two-finger pan and pinch are exercised in model space, in a tiled view, on
a paper sheet, and inside a floating viewport in MSPACE.
