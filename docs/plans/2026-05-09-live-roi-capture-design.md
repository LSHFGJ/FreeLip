# Live ROI Capture Design

## Goal

Turn the active Windows/WebView camera preview into a real local ROI clip request that can be posted to `/decode`, without treating fixture responses or preview-only camera access as recognition.

## Findings

- The sidecar schema accepts `roi.local_ref` values with a `local://` prefix.
- The real CNVSRC2025 adapter is stricter than the schema: it expects `local://path/<path>` or `local://file/<path>` and the referenced media file must exist.
- Browser canvas or `MediaStream` memory cannot be referenced directly by the Python runtime adapter.
- The existing `/decode` client and `runDecodeWhenReady()` already prevent fixture/non-real responses from dispatching candidates.

## Design

1. Add a protected local sidecar endpoint, `POST /roi/clips`, for loopback-only ROI clip ingest.
2. The frontend records a short normalized camera clip from the preview video/canvas using `MediaRecorder` and posts it to `/roi/clips` as base64 JSON.
3. The sidecar writes the bytes to a local temp ROI clip store and returns a schema-valid `RoiRequest` whose `roi.local_ref` is adapter-compatible (`local://path/...`).
4. The frontend passes that returned `RoiRequest` into `runDecodeWhenReady()` with the existing `/decode` client.
5. Quality flags stay conservative until landmark cropping exists: the first MVP can decode a normalized live camera clip, but it must mark missing face/mouth/crop evidence as low quality and never auto-insert.

## Non-goals

- Do not send raw ROI/video bytes in `/decode` JSON.
- Do not commit ROI clips, screenshots, model files, or local adapter code.
- Do not claim mouth-landmark ROI quality until landmark-based cropping is implemented.

## Success criteria

- Camera preview can produce a local ROI clip file through sidecar ingest.
- `/decode` receives an adapter-compatible `local_ref` pointing at an existing local file.
- UI no longer stays blocked at `WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED` when MediaRecorder/canvas capture is available.
- Fixture or non-real decode responses still dispatch no recognition candidates.
