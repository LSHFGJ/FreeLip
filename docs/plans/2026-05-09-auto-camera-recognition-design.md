# Auto Camera Recognition Design

## Goal

- Remove the manual camera request button.
- Attempt camera access automatically after the Windows debug app opens.
- Wire the UI toward real CNVSRC `/decode` recognition without claiming fixture or preview-only behavior is real inference.

## Current facts

- `MODEL_READY` proves the CNVSRC sidecar/runtime is available; it does not prove webcam frames are being recognized.
- `src/main.ts` only polls `SIDECAR_STATUS_URL` (`/model/status`).
- `src/cameraProbe.ts` only calls `navigator.mediaDevices.getUserMedia` for local preview.
- No frontend code currently posts to `/decode`.
- `python/freelip_vsr/sidecar.py` implements `/decode` and validates ROI-shaped JSON payloads.
- `/decode` expects `schemas/roi_request.schema.json`; it returns `schemas/candidate_response.schema.json`.
- Tauri/WebView2 can attempt `getUserMedia` on startup, but camera access still depends on Windows/WebView2/user permission.

## Design

### A. Auto-start camera preview

- Refactor `createCameraProbeController` to expose a `start()` method.
- Register the existing click handler to call the same `start()` method for retry/debug use.
- In `src/main.ts`, call `start()` once after the camera probe elements are attached.
- Remove or hide the primary “Request camera preview” button from normal UI copy.
- Keep passive status messages for permission denied, unavailable camera, busy camera, and successful local preview.
- Do not claim camera permission can be bypassed.

### B. Real `/decode` recognition bridge

- Add a frontend sidecar decode client that posts ROI request JSON to `/decode` with `Authorization: Bearer ${DEBUG_SIDECAR_TOKEN}` and `X-FreeLip-Token`.
- Add typed request/response shaping based on `roi_request.schema.json` and `candidate_response.schema.json`.
- Convert `/decode` candidates into `OverlayCandidate[]` and feed the existing `ProcessingComplete` event.
- Treat fixture responses, unavailable sidecar responses, and malformed responses as non-real recognition.
- If live ROI data is not yet available, surface an honest `WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED` / ROI-not-ready message instead of sending fake recognition.

## Phasing

1. Implement A first with TDD against `scripts/test_ui.ts`.
2. Add the `/decode` transport client and response mapping with unit tests.
3. Add a recognition orchestrator that starts only when both camera stream and real model readiness are available.
4. Keep a clear blocker/status path for missing live ROI capture.
5. Add Windows manual verification for permission prompt, preview, sidecar `MODEL_READY`, and `/decode` behavior.

## Risks

- WebView2 may prompt or deny camera access on startup; the UI must explain this without a manual request button.
- Current schemas use `roi.local_ref` rather than raw media bytes, so a real ROI producer is required before true camera-frame recognition can be claimed.
- The existing Rust core has fixture/full-loop models but no live frontend-to-sidecar decode transport.
- Mock/fixture decode must remain labeled as non-real.

## Success criteria

- App launch automatically attempts camera preview.
- UI no longer asks the user to click “Request camera preview.”
- Real model readiness remains visible and separate from camera/recognition status.
- `/decode` client is tested with real contract-shaped JSON.
- No UI path reports real recognition unless `/decode` returns real, non-fixture candidates.
