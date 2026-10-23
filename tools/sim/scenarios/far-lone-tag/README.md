# Far lone tag

One FRC tag (id 7, 0.1651 m) at x = 5 m facing the camera. The camera starts 1.5 m from it, backs
off to 3.5 to 4 m (about 23 to 27 px across at f = 566 px) a few degrees off its axis, then turns
away twice so the tag leaves the frame (a gap with no motion prediction) and comes back on it far
away, as in Eidos aaba2e94's single-tag case. The trajectory loops every 32 s.

Run it with `tools/sim/scenario.sh far-lone-tag`: the sim with this layout and camera path, the
live-map template with this layout, and a per-frame report of the pose against the truth (error,
jumps, the tag's size in pixels and the viewing angle).
