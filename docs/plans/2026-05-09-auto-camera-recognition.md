# Auto Camera Recognition Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Auto-start camera access after launch and connect the frontend to the real CNVSRC sidecar `/decode` path without presenting preview-only or fixture behavior as recognition.

**Architecture:** Split camera permission/preview, model readiness, and decode transport into separate states. Reuse `src/cameraProbe.ts` for startup camera acquisition, add a typed sidecar decode client, and add an orchestrator that only reports recognition after a real `/decode` candidate response. Keep ROI-not-ready as an honest blocker until live ROI data exists.

**Tech Stack:** TypeScript frontend, DOM `navigator.mediaDevices.getUserMedia`, Tauri WebView2, Python sidecar HTTP API, `schemas/roi_request.schema.json`, `schemas/candidate_response.schema.json`, `scripts/test_ui.ts`.

---

## Ground rules

- Do not use fixture/mock output as real recognition.
- Do not claim Windows camera permission can be bypassed.
- Do not commit unless the user explicitly asks.
- Keep unrelated existing changes out of this branch.
- Use TDD: write the failing test, run it, implement the minimum code, rerun.

### Task 1: Refactor camera probe to expose `start()`

**Files:**
- Modify: `src/cameraProbe.ts`
- Test: `scripts/test_ui.ts`

**Step 1: Write the failing test**

Add a test after `runCameraProbeTests()`:

```ts
async function runCameraProbeStartMethodTests() {
  const calls: MediaStreamConstraints[] = [];
  const stream = { id: "camera-stream" } as MediaStream;
  const button = new MockButton();
  const status = new MockStatus();
  const video = new MockVideo();

  const controller = createCameraProbeController({
    button,
    status,
    video,
    getMediaDevices: () => ({
      getUserMedia: async (constraints: MediaStreamConstraints) => {
        calls.push(constraints);
        return stream;
      }
    })
  });

  await controller.start();

  deepStrictEqual(calls, [{ video: true, audio: false }]);
  strictEqual(video.srcObject, stream);
  strictEqual(status.className, "camera-status camera-status-ready");
}
```

Call it from `runAllTests()`.

**Step 2: Run test to verify it fails**

Run: `npm run test:ui`

Expected: FAIL because `createCameraProbeController(...)` currently returns `void` and has no `.start()`.

**Step 3: Implement minimal code**

Change `src/cameraProbe.ts` so `createCameraProbeController` returns an object:

```ts
export type CameraProbeController = {
  start: () => Promise<void>;
};

export function createCameraProbeController(...): CameraProbeController {
  async function start() {
    // existing button-click body
  }

  button.addEventListener("click", start);
  return { start };
}
```

**Step 4: Run test to verify it passes**

Run: `npm run test:ui`

Expected: PASS.

### Task 2: Auto-start camera preview once after launch

**Files:**
- Modify: `src/main.ts`
- Test: `scripts/test_ui.ts`

**Step 1: Write the failing test**

Extract a tiny pure helper so startup gating can be tested:

```ts
import { shouldAutoStartCamera } from "../src/cameraProbe.ts";

function runCameraAutoStartGateTests() {
  strictEqual(shouldAutoStartCamera(false, true), true);
  strictEqual(shouldAutoStartCamera(true, true), false);
  strictEqual(shouldAutoStartCamera(false, false), false);
}
```

Call it from `runAllTests()`.

**Step 2: Run test to verify it fails**

Run: `npm run test:ui`

Expected: FAIL because `shouldAutoStartCamera` does not exist.

**Step 3: Implement minimal code**

Add to `src/cameraProbe.ts`:

```ts
export function shouldAutoStartCamera(hasAutoStarted: boolean, canStart: boolean): boolean {
  return canStart && !hasAutoStarted;
}
```

In `src/main.ts`:

```ts
let cameraAutoStarted = false;

// inside attachEvents(), after createCameraProbeController(...)
const controller = createCameraProbeController(...);
if (shouldAutoStartCamera(cameraAutoStarted, true)) {
  cameraAutoStarted = true;
  void controller.start();
}
```

**Step 4: Update UI copy**

Replace the camera probe text with startup-oriented copy:

- Heading: `Camera Recognition`
- Body: `FreeLip starts the local camera automatically after launch. Windows/WebView2 may still ask for camera permission.`
- Remove the visible “Request camera preview” button from normal layout or render it as a hidden retry control only when an error occurs.
- Initial status: `Starting camera preview... Grant camera permission if Windows asks.`

**Step 5: Run tests**

Run: `npm run test:ui`

Expected: PASS.

### Task 3: Add sidecar decode URL/config

**Files:**
- Modify: `src/sidecarConfig.ts`
- Test: `scripts/test_ui.ts`

**Step 1: Write failing test**

Add a config assertion near existing URL tests:

```ts
import { SIDECAR_DECODE_URL } from "../src/sidecarConfig.ts";

strictEqual(SIDECAR_DECODE_URL, "http://127.0.0.1:18765/decode");
```

**Step 2: Run test to verify it fails**

Run: `npm run test:ui`

Expected: FAIL because `SIDECAR_DECODE_URL` does not exist.

**Step 3: Implement minimal code**

Add to `src/sidecarConfig.ts`:

```ts
export const SIDECAR_DECODE_URL = `http://${SIDECAR_HOST}:${SIDECAR_PORT}/decode`;
```

**Step 4: Run test**

Run: `npm run test:ui`

Expected: PASS.

### Task 4: Add typed decode client

**Files:**
- Create: `src/sidecarDecode.ts`
- Test: `scripts/test_ui.ts`

**Step 1: Write failing test**

Add a test that passes a fake fetch and verifies method, headers, body, and mapped candidates:

```ts
import { decodeRoiWithSidecar } from "../src/sidecarDecode.ts";

async function runSidecarDecodeClientTests() {
  const requests: RequestInit[] = [];
  const payload = createTestRoiRequest();
  const response = createTestCandidateResponse();

  const result = await decodeRoiWithSidecar(payload, {
    url: "http://127.0.0.1:18765/decode",
    token: "test-token",
    fetchJson: async (_url, init) => {
      requests.push(init);
      return response;
    }
  });

  strictEqual(requests[0].method, "POST");
  strictEqual((requests[0].headers as Record<string, string>).Authorization, "Bearer test-token");
  strictEqual(result.candidates[0].text, response.candidates[0].text);
  strictEqual(result.realDecode, true);
}
```

**Step 2: Run test to verify it fails**

Run: `npm run test:ui`

Expected: FAIL because `src/sidecarDecode.ts` does not exist.

**Step 3: Implement minimal code**

Create `src/sidecarDecode.ts` with:

```ts
export type RoiRequest = { /* fields from schemas/roi_request.schema.json */ };
export type CandidateResponse = { /* fields from schemas/candidate_response.schema.json */ };
export type DecodeClientOptions = {
  url: string;
  token: string;
  fetchJson?: (url: string, init: RequestInit) => Promise<unknown>;
};

export async function decodeRoiWithSidecar(request: RoiRequest, options: DecodeClientOptions) {
  const fetchJson = options.fetchJson ?? defaultFetchJson;
  const payload = await fetchJson(options.url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${options.token}`,
      "X-FreeLip-Token": options.token
    },
    body: JSON.stringify(request)
  });
  return mapCandidateResponse(payload);
}
```

Keep validation lightweight: verify required top-level fields and candidates array. Do not import heavy schema validators into the frontend unless needed.

**Step 4: Run tests**

Run: `npm run test:ui`

Expected: PASS.

### Task 5: Add honest live-camera recognition orchestrator state

**Files:**
- Create: `src/cameraRecognition.ts`
- Modify: `src/main.ts`
- Test: `scripts/test_ui.ts`

**Step 1: Write failing tests**

Test three behaviors:

```ts
import { createCameraRecognitionStatus, mapDecodeCandidatesToOverlay } from "../src/cameraRecognition.ts";

function runCameraRecognitionStatusTests() {
  strictEqual(
    createCameraRecognitionStatus({ cameraReady: true, modelReady: true, roiReady: false }).code,
    "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED"
  );
}

function runCandidateMappingTests() {
  const mapped = mapDecodeCandidatesToOverlay(createTestCandidateResponse().candidates);
  strictEqual(mapped[0].source, "cnvsrc2025");
}
```

**Step 2: Run test to verify it fails**

Run: `npm run test:ui`

Expected: FAIL because `src/cameraRecognition.ts` does not exist.

**Step 3: Implement minimal code**

Create `src/cameraRecognition.ts`:

```ts
export function createCameraRecognitionStatus(input: {
  cameraReady: boolean;
  modelReady: boolean;
  roiReady: boolean;
}) {
  if (!input.cameraReady) return { code: "CAMERA_NOT_READY", realRecognition: false };
  if (!input.modelReady) return { code: "MODEL_NOT_READY", realRecognition: false };
  if (!input.roiReady) {
    return {
      code: "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED",
      realRecognition: false,
      message: "Camera preview is active, but live ROI capture is not wired to /decode yet."
    };
  }
  return { code: "READY_TO_DECODE", realRecognition: true };
}

export function mapDecodeCandidatesToOverlay(candidates: Array<{ text: string; source: string }>) {
  return candidates.slice(0, 5).map((candidate) => ({ text: candidate.text, source: candidate.source }));
}
```

**Step 4: Wire main UI status**

Add a camera recognition status section in `src/main.ts` separate from model status.

Do not dispatch `ProcessingComplete` until a real ROI request exists and `/decode` succeeds.

**Step 5: Run tests**

Run: `npm run test:ui`

Expected: PASS.

### Task 6: Add real decode execution path once ROI producer exists

**Files:**
- Modify: `src/main.ts`
- Modify: `src/cameraRecognition.ts`
- Test: `scripts/test_ui.ts`

**Step 1: Write failing test**

Add a test for the orchestrator:

```ts
async function runRecognitionDecodeDispatchTests() {
  const events: AppEvent[] = [];
  await runDecodeWhenReady({
    cameraReady: true,
    modelReady: true,
    roiRequest: createTestRoiRequest(),
    decode: async () => createTestCandidateResponse(),
    dispatch: (event) => events.push(event)
  });

  strictEqual(events[0].type, "ProcessingComplete");
}
```

**Step 2: Run test to verify it fails**

Run: `npm run test:ui`

Expected: FAIL because `runDecodeWhenReady` does not exist.

**Step 3: Implement minimal code**

Add `runDecodeWhenReady` that:

- returns early if camera/model/ROI is not ready.
- calls `decodeRoiWithSidecar` only when a real `roiRequest` exists.
- dispatches `ProcessingComplete` with mapped candidates.
- treats thrown errors as non-real recognition status.

**Step 4: Run tests**

Run: `npm run test:ui`

Expected: PASS.

### Task 7: Update docs and debug bundle contract

**Files:**
- Modify: `README.md`
- Modify: `docs/internal-mvp.md`
- Modify: `scripts/test_debug_bundle.mjs`

**Step 1: Write failing tests**

Add assertions to `scripts/test_debug_bundle.mjs`:

```js
assertContains("src/sidecarConfig.ts", "/decode", "frontend sidecar decode endpoint config");
assertContains("src/main.ts", "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED", "honest ROI integration blocker when preview is not enough");
assertNotContains("src/main.ts", "Request camera preview", "manual camera request button copy");
```

**Step 2: Run test to verify it fails**

Run: `npm run test:debug-bundle`

Expected: FAIL until code/docs are updated.

**Step 3: Update docs**

Document:

- Auto camera startup attempts on launch.
- Windows/WebView2 permission may still prompt or deny.
- `MODEL_READY` is separate from live camera recognition.
- Live recognition requires real ROI data sent to `/decode`.

**Step 4: Run tests**

Run: `npm run test:debug-bundle`

Expected: PASS.

### Task 8: Final verification

**Files:**
- All modified files

**Step 1: Run targeted UI tests**

Run: `npm run test:ui`

Expected: all tests pass.

**Step 2: Run debug bundle contract tests**

Run: `npm run test:debug-bundle`

Expected: all tests pass.

**Step 3: Run build**

Run: `npm run build`

Expected: TypeScript and Vite build pass.

**Step 4: Run Python sidecar tests**

Run: `PYTHONPATH=/home/lshfgj/FreeLip/python .venv/bin/python -m pytest python/tests -q`

Expected: existing Python sidecar tests pass.

**Step 5: Run git diff hygiene**

Run: `GIT_MASTER=1 git diff --check`

Expected: no output.

**Step 6: Manual Windows verification**

Run in Windows debug bundle after building:

```powershell
npm run bundle:debug:win
powershell -NoProfile -ExecutionPolicy Bypass -File .\debug-dist\FreeLip-debug\run-debug.ps1
```

Expected:

- camera preview attempts automatically after app opens.
- Windows/WebView2 permission prompt appears if permission is not persisted.
- `/model/status` still reports real model readiness when sidecar is ready.
- UI does not claim real recognition until `/decode` returns non-fixture candidates.

**Step 7: Git status**

Run: `GIT_MASTER=1 git status --short --branch`

Expected: only intended files changed. Do not commit unless the user explicitly requests it.
