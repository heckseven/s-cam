# Shelved ideas, with the reasoning

Sized up and deliberately not built. Recorded so the feasibility work is not repeated, and so
the reasons are available when someone revisits them.

See also ANIMATED-BLING.md, which has its own numbers.

## Morse code detection via the camera

**Transmitting** is easy — the LED ring is already programmable.

**Receiving** is bounded by three things:

* **Sampling rate.** A Morse dot is `1200/WPM` ms, and telling a dot from a dash wants about
  five samples per dot: 5 WPM needs ~20fps, 10 WPM ~40fps, 20 WPM ~80fps.
* **Frame cost.** Shipping a 256x240 frame to the app to measure brightness is far too
  expensive per sample. Brightness must be computed inside `bao-video`, where the frame
  already is, and returned as a single number.
* **Auto-exposure, which is the real obstacle.** The GC2145 runs with AEC enabled (settings
  taken from the Linux kernel). It adapts to scene brightness, so a blinking source makes the
  sensor hunt: flashes get darkened, gaps brightened, and the contrast being measured is
  actively suppressed — with lag on a timescale close to Morse itself. Fixing exposure and
  gain manually is likely a precondition, and the driver does not expose that today.

**Plausible at 10 WPM or slower, not at speed.** If picked up: do not write a decoder first.
Add a brightness opcode to `bao-video` and measure the achievable sample rate and whether AEC
swamps the signal. Those two numbers decide everything and are cheap to get.

## Symbol cipher recognition via the camera

There is no vision pipeline to build on. QR decoding is done by the `rqrr` crate — perspective
correction, grid extraction, error correction. The badge's own `qr.rs` is 244 lines that find
finder patterns to draw a crosshair, nothing more.

Would need: binarise (precedent exists), connected-component segmentation, then template
matching of each glyph normalised to about 16x16. For a fixed known symbol set that is
1980s computer vision, not machine learning.

**Budget is not the constraint** - 26 templates at 16x16 is under 1KB, code maybe 5-10KB
against roughly 116KB of headroom. The constraints are optical:

* **Rotation.** Pigpen-style glyphs are orientation-sensitive; `⌐` and `L` differ only by
  rotation. Camera tilt breaks naive matching, and rotation-invariant matching discards the
  very information that separates those glyphs.
* **Auto-exposure and no focus control**, as above.
* **Hand-drawn variance and touching glyphs**, which is where segmentation quietly fails.

**Achievable for cleanly printed, well-lit, upright, known symbol sets. Not in the wild.**

**Cheaper alternative worth considering first:** a cipher *solver* rather than a *recogniser* -
type the ciphertext at the REPL or scan a QR containing it, and decode substitution, Caesar,
Vigenere, frequency analysis. No optical risk, a fraction of the work, and it covers the
actual puzzle-solving use. The camera-based glyph recognition is the expensive part that
fails in bad light.
