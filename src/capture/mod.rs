// TODO: implement DXGI Desktop Duplication screen capture (Windows only).
// Use windows-rs with IDXGIOutputDuplication:
//   1. Create D3D11 device + DXGI factory.
//   2. Enumerate adapter outputs, call IDXGIOutput1::DuplicateOutput.
//   3. AcquireNextFrame → map the staging texture → copy BGRA pixels.
//   4. Convert BGRA → RGB, wrap in image::RgbImage, pass to pipeline::process_frame.
//   5. ReleaseFrame, loop at ~2 fps (500ms sleep between frames).
//
// Target: <1% CPU impact. DXGI gives GPU-side copy; the staging map is the only CPU copy.
