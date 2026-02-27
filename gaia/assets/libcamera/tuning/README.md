# Libcamera tuning (Helios)

These files are intended to ensure `ov9782` can produce usable color output without relying on
whatever the target image happens to ship.

Currently, `ov9782.json` is intentionally based on Raspberry Pi's known-good `imx219` tunings to
avoid libcamera IPA crashes on some stream formats. This is a stability-first fallback until a
proper OV9782 calibration (CCM/AWB/ALSC/noise model) is generated.

For OV9782, we stage repo-owned tuning files into the image during post-build from:

- `assets/libcamera/ipa/rpi/pisp/ov9782.json` (Pi 5 / PISP)
- `assets/libcamera/ipa/rpi/vc4/ov9782.json` (Pi 4/older / BCM2835 VC4 pipeline)

Post-build intentionally does not derive/merge tuning files; update these files directly.

This is not a substitute for real calibration (CCM, AWB priors, lens shading, noise model).
