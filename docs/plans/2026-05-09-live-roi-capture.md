# Live ROI Capture Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Capture a short live camera clip locally, materialize it as an adapter-compatible `local://path/...` ROI file, and feed the returned `RoiRequest` into the existing real `/decode` path.

**Architecture:** Add a protected `/roi/clips` sidecar ingest endpoint that writes local clip bytes to temp storage and returns a full `RoiRequest`. Add a frontend ROI producer that records a normalized clip from the preview video/canvas, posts it to `/roi/clips`, then calls `runDecodeWhenReady()` with the returned request.

**Tech Stack:** TypeScript frontend, WebView2 `MediaRecorder`, Python sidecar HTTP API, base64 JSON ingest, existing `/decode` client and candidate dispatch state machine.

---

### Task 1: Sidecar ROI clip ingest endpoint

**Files:**
- Modify: `python/freelip_vsr/sidecar.py`
- Test: `python/tests/test_sidecar_api.py`

**Steps:**
1. Add a failing test that posts to `/roi/clips` with a tiny base64 WebM payload.
2. Assert missing/wrong token fails before writing files.
3. Assert a valid request writes a file and returns `roi_request.roi.local_ref` beginning with `local://path/`.
4. Implement endpoint validation, local temp writing, and conservative quality flag passthrough.
5. Run `python -m pytest python/tests/test_sidecar_api.py -q`.

### Task 2: Frontend ROI ingest client and recorder producer

**Files:**
- Create: `src/sidecarRoi.ts`
- Modify: `src/sidecarConfig.ts`
- Test: `scripts/test_ui.ts`

**Steps:**
1. Add failing tests for `SIDECAR_ROI_CLIP_URL`, ingest headers/body, returned `RoiRequest`, conservative quality flags, and live ROI readiness gate.
2. Implement typed `ingestRoiClipWithSidecar()` and `createDefaultRoiQualityFlags()`.
3. Implement `canCaptureLiveRoi()` and a recorder abstraction for `MediaRecorder`/canvas capture.
4. Run `npm run test:ui`.

### Task 3: Wire recording stop to ROI ingest and `/decode`

**Files:**
- Modify: `src/main.ts`
- Test: `scripts/test_ui.ts`

**Steps:**
1. Add testable helper(s) so hotkey stop can build ROI then decode.
2. On `RecordingStopped`, enter `Processing`, capture/ingest ROI, call `runDecodeWhenReady()`, and recover to idle with an honest toast on capture/decode errors.
3. Update recognition status to distinguish capture availability from decode completion.
4. Run UI/build/Python verification.
