# Status ring (LEDs)

The Raze has a 16-LED ring. HeliOS shows its state on it through lemnosd (the Lemnos board
service, which owns the hardware). The ring is read at a glance: cool colours are healthy, warm
colours mean trouble, and motion means looking.

## The states

The highest state that applies is shown. `locate` is timed; the others follow the device.

| State | Shows | Look |
|---|---|---|
| Locate | A soft rainbow comet, for 10 seconds (from the Locate button) | drawn by helios-api |
| Error or camera lost | Red breath, slow | `helios.error` |
| Calibration capture | Green arc for the coverage of the captured views; a green ripple from the top on each captured view | `helios.calibrating`, `helios.calibrating-view` |
| Robot enabled | A one-shot cyan ripple when the robot is enabled; the ring is 30% brighter while it is enabled | `helios.robot-enabled` |
| Robot code not connected | Amber twin orbit (team number set, NetworkTables not connected) | `helios.robot-wait` |
| Robot not connected, tags visible | Amber orbit over a 25% cyan glow | `helios.robot-wait-tags` |
| Tags visible | Cyan breathing glow, with one bright spot per tag at its bearing | `helios.tags` |
| Field pose locked | The spots warm toward white, and a slow white sheen orbits | `helios.tags-locked` |
| Searching | Violet comet orbit with a dim glow (a pipeline is running, no tags) | `helios.searching` |
| Starting | White sparkle over a slow breath, cross-fading into the steady state | `helios.starting` |
| Recording | A soft red tally on LED 0 over whatever else shows (an option) | `helios.rec.*`, `helios.recording` |
| Idle | Nothing from HeliOS (a pipeline is not running, and the robot is not wanted) | none |

Every comet keeps a dim glow of its own colour (0.18). Nothing blinks or flashes except the
one-shot ripples.

Colours (the approved scheme B): cyan `28c8ff`, violet `965aff`, amber `ff9614`, red `ff2828`,
green `2bd47d` (calibration), white for starting and the locked sheen.

## How a tag gets its place on the ring

The tag centre's x in the frame gives its bearing from the camera's forward axis:

- with a calibration for the frame's size (or the same aspect ratio), the ray through the centre is
  undistorted (Brown-Conrady, or equidistant for a fisheye) and its horizontal angle taken;
- without one, the centre's x fraction times the horizontal field of view (`fov_deg`, default 90).

Camera forward is the logical LED `forward_led` (0 is the top); a positive bearing (right of the
centre) runs clockwise when `direction` is `cw`. A tag's spot is brighter the closer (larger) it
is, and glides to a new place. Logical indices already include the board's `offset` and
`direction` (`docs/board-definition.md` in Lemnos), so nothing else is needed.

## Settings

Stored by helios-api (`leds.json`), read and written at `GET`/`PUT /v1/device/leds`:

| Setting | Default | Meaning |
|---|---|---|
| `enabled` | `true` | HeliOS drives the ring. Off releases it. |
| `brightness` | `0.5` | Ring brightness, 0 to 1 (lemnosd's ring-wide scale; 0.5 is the board's default). |
| `ambient.enabled`, `ambient.min`, `ambient.max` | off, `0.3`, `1.0` | Scale the brightness from the camera's lux (log scale, full at 1000 lux), smoothed over about 3 seconds. |
| `forward_led` | `0` | The logical LED that faces the camera's centre (0 to 15). |
| `direction` | `cw` | `cw` or `ccw`: which way positive bearings run. |
| `fov_deg` | `90` | Horizontal field of view for tag bearings until the camera is calibrated (20 to 180). |
| `tally` | `true` | Recording tally on LED 0. |
| `boost` | `true` | Brightness +30% while the robot is enabled. |

`POST /v1/device/leds/locate` (optional `{"seconds": 1..60}`, default 10) shows the locate state.
`POST /v1/device/leds/preview` (`{"state": "<state>" | null, "seconds": 1..30}`) shows a state for a
few seconds; `null` ends it. The `states` list in the response names every state, with its look.

The live state is in the `leds` event (about 10 a second): the state, the look, the spots, and,
when HeliOS draws the frame itself, the 16 colours.

## Looks

HeliOS's looks are a look file in the image, `/etc/lemnos/looks.d/helios.toml` (source:
`gaia/assets/runtime-config/etc/lemnos/looks.d/helios.toml`). lemnosd reads it; a look file
overrides a built-in of the same name. The `helios.rec.*` looks are the same states with the
tally layer added. A test checks that every look a state names is defined, and every defined look
is named by a state.

Two things are drawn by helios-api as frames: the rainbow locate, and the tag spots when lemnosd
has no spots block (an older lemnosd). The spots themselves use lemnosd's spots block: helios-api
sends their positions with `look_spots` and `update_inputs`, and lemnosd glides them.

## Known gaps

- The robot's enable state and the camera's lux are not published by helios-engine yet. helios-api
  reads them from the engine's runtime resource labels `helios.robot.enabled=true` and
  `helios.camera.lux=<lux>`; until they appear, the enable ripple and the ambient brightness do not
  trigger.
- The `leds` event carries the 16 colours of the tags' fallback frame, so the preview shows that
  frame, not the device's spots block.

## Priority on the ring

lemnosd arbitrates the status ring by layer and priority. helios-api's director is an application
client at priority 160. helios-peripherals' health status sends the built-in `status.*` look at
priority 20, so it shows only while the API is down (or the director is off), and lemnosd drops
the director's looks when helios-api stops.
