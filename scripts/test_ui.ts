import { createCameraProbeController, shouldAutoStartCamera } from "../src/cameraProbe.ts";
import {
  createCameraRecognitionStatus,
  mapDecodeCandidatesToOverlay,
  runLiveRoiDecodeWhenReady,
  runDecodeWhenReady
} from "../src/cameraRecognition.ts";
import { shouldShowDevControls } from "../src/devMode.ts";
import type { AppEvent } from "../src/hotkeyState.ts";
import { reduce } from "../src/hotkeyState.ts";
import { escapeModelStatusText, formatModelStatus } from "../src/modelStatus.ts";
import { renderCandidates } from "../src/render.ts";
import {
  canCaptureLiveRoi,
  createDefaultRoiQualityFlags,
  ingestRoiClipWithSidecar,
  prepareCameraRoiRequest
} from "../src/sidecarRoi.ts";
import type { CandidateResponse, RoiRequest } from "../src/sidecarDecode.ts";
import { decodeRoiWithSidecar } from "../src/sidecarDecode.ts";
import { SIDECAR_DECODE_URL, SIDECAR_ROI_CLIP_URL } from "../src/sidecarConfig.ts";

function strictEqual(actual: unknown, expected: unknown) {
  if (!Object.is(actual, expected)) {
    throw new Error(`Expected ${String(actual)} to strictly equal ${String(expected)}`);
  }
}

function deepStrictEqual(actual: unknown, expected: unknown) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`Expected ${actualJson} to deeply equal ${expectedJson}`);
  }
}

function ok(value: unknown, message: string) {
  if (!value) {
    throw new Error(message);
  }
}

class MockElement {
  children: unknown[] = [];
  className = "";
  textContent = "";
  innerHTML = "";
  attributes: Record<string, string> = {};
  tagName: string;

  constructor(tagName: string) {
    this.tagName = tagName;
  }

  setAttribute(name: string, value: string) {
    this.attributes[name] = value;
  }
  
  appendChild(child: unknown) {
    this.children.push(child);
  }
}

class MockButton {
  disabled = false;
  private clickListener: (() => void | Promise<void>) | null = null;

  addEventListener(event: string, listener: () => void | Promise<void>) {
    if (event === "click") {
      this.clickListener = listener;
    }
  }

  async click() {
    await this.clickListener?.();
  }
}

class MockStatus {
  textContent = "";
  className = "";
}

class MockVideo {
  srcObject: MediaStream | null = null;
  hidden = true;
  muted = false;
  playsInline = false;
}

function expectMockElement(value: unknown): MockElement {
  if (value instanceof MockElement) {
    return value;
  }

  throw new Error("Expected a MockElement child");
}

const mockApp = new MockElement("div");
Object.defineProperty(globalThis, "document", {
  value: {
  querySelector: (sel: string) => {
    if (sel === "#app") return mockApp;
    return null;
  },
  querySelectorAll: () => [],
  createElement: (tag: string) => new MockElement(tag),
  createTextNode: (text: string) => ({ textContent: text, isTextNode: true }),
  addEventListener: () => {},
  body: new MockElement("body"),
  },
  configurable: true
});

function runTests() {
  // Test Idle
  let result = reduce({ type: "Idle", chord: "Ctrl+Alt+Space" }, { type: "HotkeyPressed" });
  deepStrictEqual(result.state, { type: "Recording", chord: "Ctrl+Alt+Space" });

  // Test Collision
  result = reduce({ type: "Idle", chord: "Ctrl+Alt+Space" }, { type: "CollisionDetected" });
  deepStrictEqual(result.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });
  
  // Test remap ignores HotkeyPressed
  result = reduce(result.state, { type: "HotkeyPressed" });
  deepStrictEqual(result.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });
  
  // Test remap ignores empty or same chord
  let remapResult = reduce({ type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" }, { type: "Remapped", newChord: "   " });
  deepStrictEqual(remapResult.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });

  remapResult = reduce({ type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" }, { type: "Remapped", newChord: "Ctrl+Alt+Space" });
  deepStrictEqual(remapResult.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });

  result = reduce(result.state, { type: "Remapped", newChord: "Ctrl+Alt+L" });
  deepStrictEqual(result.state, { type: "Idle", chord: "Ctrl+Alt+L" });
  
  // Now starts recording with new chord
  result = reduce(result.state, { type: "HotkeyPressed" });
  deepStrictEqual(result.state, { type: "Recording", chord: "Ctrl+Alt+L" });

  // Test full flow
  result = reduce({ type: "Idle", chord: "Ctrl+Alt+Space" }, { type: "HotkeyPressed" });
  result = reduce(result.state, { type: "RecordingStopped" });
  deepStrictEqual(result.state, { type: "Processing", chord: "Ctrl+Alt+Space" });
  
  const candidates = [
    { text: "C1", source: "vsr" },
    { text: "C2", source: "llm" },
    { text: "C3", source: "vsr" },
    { text: "C4", source: "vsr" },
    { text: "C5", source: "vsr" },
    { text: "C6", source: "vsr" }
  ];
  result = reduce(result.state, { type: "ProcessingComplete", candidates, lowQuality: true, autoInsertThresholdMet: false });
  // Should truncate to 5
  deepStrictEqual(result.state, { type: "ShowingCandidates", chord: "Ctrl+Alt+Space", candidates: candidates.slice(0, 5), lowQuality: true, autoInsertThresholdMet: false });
  
  // Select first candidate
  const selectResult = reduce(result.state, { type: "NumberKeyPressed", index: 1 });
  deepStrictEqual(selectResult.state, { type: "Idle", chord: "Ctrl+Alt+Space" });
  deepStrictEqual(selectResult.action, { type: "InsertCandidate", candidate: candidates[0] });

  // Test Escape from Candidates
  result = reduce({ type: "ShowingCandidates", chord: "Ctrl+Alt+Space", candidates: candidates.slice(0, 5), lowQuality: false, autoInsertThresholdMet: false }, { type: "EscapePressed" });
  deepStrictEqual(result.state, { type: "Idle", chord: "Ctrl+Alt+Space" });
  deepStrictEqual(result.action, { type: "Cancel" });

  // Test Escape from Recording
  result = reduce({ type: "Recording", chord: "Ctrl+Alt+L" }, { type: "EscapePressed" });
  deepStrictEqual(result.state, { type: "Idle", chord: "Ctrl+Alt+L" });
  deepStrictEqual(result.action, { type: "Cancel" });

  const container = new MockElement("ol");
  const maliciousCandidates = [
    { text: "<img src=x onerror=alert(1)>", source: "<b>llm</b>" }
  ];
  renderCandidates(container as unknown as Element, maliciousCandidates);
  
  strictEqual(container.innerHTML, "");
  strictEqual(container.children.length, 1);
  
  const firstChild = expectMockElement(container.children[0]);
  strictEqual(firstChild.tagName, "li");
  strictEqual(firstChild.children.length, 5);
  
  strictEqual(expectMockElement(firstChild.children[0]).textContent, "1.");
  strictEqual(expectMockElement(firstChild.children[2]).textContent, "<img src=x onerror=alert(1)>");
  strictEqual(expectMockElement(firstChild.children[4]).textContent, "<b>llm</b>");
}

function runModelStatusTests() {
  const fixtureStatus = formatModelStatus({
    backend: "fixture",
    status: "FIXTURE_MODE",
    fixture_mode: true,
    fallback_active: true,
    ready: true,
    model_id: "cnvsrc2025",
    runtime_id: "cnvsrc2025-fixture-cpu-checkpoint-gated"
  });

  ok(
    fixtureStatus.text.toLowerCase().includes("fixture") || fixtureStatus.text.toLowerCase().includes("mock"),
    `Expected fixture status text to disclose non-real backend, got: ${fixtureStatus.text}`
  );
  ok(
    fixtureStatus.text.toLowerCase().includes("not real") || fixtureStatus.text.toLowerCase().includes("非真实"),
    `Expected fixture status text to say it is not real model inference, got: ${fixtureStatus.text}`
  );
  strictEqual(fixtureStatus.tone, "warning");
  strictEqual(fixtureStatus.realModelReady, false);

  const realStatus = formatModelStatus({
    backend: "cnvsrc2025",
    status: "READY",
    fixture_mode: false,
    fallback_active: false,
    ready: true,
    model_id: "cnvsrc2025",
    runtime_id: "cnvsrc2025-official-cuda"
  });

  strictEqual(realStatus.tone, "ready");
  strictEqual(realStatus.realModelReady, true);
  ok(
    realStatus.text.includes("cnvsrc2025-official-cuda"),
    `Expected real status text to include runtime id, got: ${realStatus.text}`
  );

  strictEqual(
    escapeModelStatusText("<img src=x onerror=alert(1)> & runtime"),
    "&lt;img src=x onerror=alert(1)&gt; &amp; runtime"
  );

  const unreachableStatus = formatModelStatus({
    backend: "cnvsrc2025",
    status: "SIDECAR_UNREACHABLE",
    error_code: "SIDECAR_UNREACHABLE",
    ready: false,
    model_id: "cnvsrc2025",
    runtime_id: "local-sidecar",
    message: "Failed to fetch"
  });

  strictEqual(unreachableStatus.tone, "error");
  strictEqual(unreachableStatus.realModelReady, false);
  ok(
    unreachableStatus.text.includes("run-debug.ps1") && unreachableStatus.text.includes("sidecar"),
    `Expected unreachable status to explain the sidecar launch path, got: ${unreachableStatus.text}`
  );
}

function runDevModeTests() {
  strictEqual(shouldShowDevControls("", null), false);
  strictEqual(shouldShowDevControls("?dev=1", null), true);
  strictEqual(shouldShowDevControls("", "1"), true);
}

async function runCameraProbeTests() {
  const calls: MediaStreamConstraints[] = [];
  const stream = { id: "camera-stream" } as MediaStream;
  const button = new MockButton();
  const status = new MockStatus();
  const video = new MockVideo();

  createCameraProbeController({
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

  await button.click();

  deepStrictEqual(calls, [{ video: true, audio: false }]);
  strictEqual(video.srcObject, stream);
  strictEqual(video.hidden, false);
  strictEqual(video.muted, true);
  strictEqual(video.playsInline, true);
  strictEqual(button.disabled, false);
  strictEqual(
    status.textContent,
    "Camera preview is active. This only verifies camera permission and local capture, not ROI cropping or model inference."
  );
  strictEqual(status.className, "camera-status camera-status-ready");
}

async function runCameraProbeStartMethodTests() {
  const calls: MediaStreamConstraints[] = [];
  const readyStreams: MediaStream[] = [];
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
    }),
    onStreamReady: (readyStream) => readyStreams.push(readyStream)
  });

  await controller.start();

  deepStrictEqual(calls, [{ video: true, audio: false }]);
  strictEqual(video.srcObject, stream);
  strictEqual(status.className, "camera-status camera-status-ready");
  deepStrictEqual(readyStreams, [stream]);
}

function runCameraAutoStartGateTests() {
  strictEqual(shouldAutoStartCamera(false, true), true);
  strictEqual(shouldAutoStartCamera(true, true), false);
  strictEqual(shouldAutoStartCamera(false, false), false);
}

function runSidecarDecodeConfigTests() {
  strictEqual(SIDECAR_DECODE_URL, "http://127.0.0.1:18765/decode");
  strictEqual(SIDECAR_ROI_CLIP_URL, "http://127.0.0.1:18765/roi/clips");
}

function createTestRoiRequest(): RoiRequest {
  return {
    schema_version: "1.0.0",
    request_id: "roi-req-0001",
    session_id: "session-0001",
    source: {
      kind: "camera",
      started_at_ms: 1_777_339_200_000
    },
    roi: {
      local_ref: "local://roi/session-0001/normalized.json",
      format: "grayscale_u8",
      width: 96,
      height: 96,
      fps: 25,
      frame_count: 75,
      duration_ms: 3000
    },
    quality_flags: {
      schema_version: "1.0.0",
      face_found: true,
      mouth_landmarks_found: true,
      crop_bounds_valid: true,
      blur_ok: true,
      brightness_ok: true,
      pose_ok: true,
      occlusion_ok: true,
      landmark_confidence: 0.91,
      rejection_reasons: []
    },
    requested_at_ms: 1_777_339_203_000
  };
}

function createTestCandidateResponse(): CandidateResponse {
  const request = createTestRoiRequest();
  return {
    schema_version: "1.0.0",
    request_id: request.request_id,
    session_id: request.session_id,
    model: {
      model_id: "cnvsrc2025",
      runtime_id: "cnvsrc2025-official-cpu",
      device: "cpu"
    },
    candidates: [
      {
        schema_version: "1.0.0",
        rank: 1,
        text: "打开会议记录",
        score: 0.87,
        source: "cnvsrc2025",
        is_auto_insert_eligible: true
      }
    ],
    quality_flags: request.quality_flags,
    timing_ms: {
      roi_received_to_first_candidate: 12,
      roi_received_to_final: 28
    },
    created_at_ms: 1_777_339_204_050
  };
}

async function runSidecarDecodeClientTests() {
  const requests: RequestInit[] = [];
  const urls: string[] = [];
  const payload = createTestRoiRequest();
  const response = createTestCandidateResponse();

  const result = await decodeRoiWithSidecar(payload, {
    url: "http://127.0.0.1:18765/decode",
    token: "test-token",
    fetchJson: async (url, init) => {
      urls.push(url);
      requests.push(init);
      return response;
    }
  });

  strictEqual(urls[0], "http://127.0.0.1:18765/decode");
  strictEqual(requests[0]?.method, "POST");
  const headers = requests[0]?.headers as Record<string, string>;
  strictEqual(headers.Authorization, "Bearer test-token");
  strictEqual(headers["X-FreeLip-Token"], "test-token");
  strictEqual(result.candidates[0]?.text, response.candidates[0]?.text);
  strictEqual(result.realDecode, true);
}

async function runSidecarRoiIngestClientTests() {
  const requests: RequestInit[] = [];
  const urls: string[] = [];
  const roiRequest = createTestRoiRequest();
  roiRequest.roi.local_ref = "local://path/C:/Users/test/AppData/Local/FreeLip/roi/session/roi.webm";

  const result = await ingestRoiClipWithSidecar({
    schema_version: "1.0.0",
    request_id: roiRequest.request_id,
    session_id: roiRequest.session_id,
    source: roiRequest.source,
    clip: {
      mime_type: "video/webm",
      data_base64: "dGlueS13ZWJt",
      width: 96,
      height: 96,
      fps: 25,
      frame_count: 75,
      duration_ms: 3000
    },
    quality_flags: roiRequest.quality_flags,
    requested_at_ms: roiRequest.requested_at_ms
  }, {
    url: "http://127.0.0.1:18765/roi/clips",
    token: "test-token",
    fetchJson: async (url, init) => {
      urls.push(url);
      requests.push(init);
      return { schema_version: "1.0.0", roi_request: roiRequest };
    }
  });

  strictEqual(urls[0], "http://127.0.0.1:18765/roi/clips");
  strictEqual(requests[0]?.method, "POST");
  const headers = requests[0]?.headers as Record<string, string>;
  strictEqual(headers.Authorization, "Bearer test-token");
  strictEqual(headers["X-FreeLip-Token"], "test-token");
  ok(String(requests[0]?.body).includes("dGlueS13ZWJt"), "ROI ingest body should include clip payload");
  strictEqual(result.roi.local_ref.startsWith("local://path/"), true);
}

function runRoiProducerQualityTests() {
  const flags = createDefaultRoiQualityFlags();
  strictEqual(flags.face_found, false);
  strictEqual(flags.mouth_landmarks_found, false);
  strictEqual(flags.crop_bounds_valid, false);
  deepStrictEqual(flags.rejection_reasons, ["face_not_found", "mouth_landmarks_missing", "crop_bounds_invalid"]);
  strictEqual(canCaptureLiveRoi({ hasCameraStream: true, hasVideoElement: true, mediaRecorderAvailable: true, canvasCaptureAvailable: true }), true);
  strictEqual(canCaptureLiveRoi({ hasCameraStream: true, hasVideoElement: true, mediaRecorderAvailable: false, canvasCaptureAvailable: true }), false);
  strictEqual(canCaptureLiveRoi({ hasCameraStream: true, hasVideoElement: true, mediaRecorderAvailable: true, canvasCaptureAvailable: false }), false);
}

async function runPrepareCameraRoiRequestTests() {
  const roiRequest = createTestRoiRequest();
  roiRequest.roi.local_ref = "local://path/C:/Users/test/AppData/Local/FreeLip/roi/session/roi.webm";
  const result = await prepareCameraRoiRequest({
    sessionId: "session-0001",
    sourceStartedAtMs: 1_777_339_200_000,
    requestedAtMs: 1_777_339_203_000,
    recordClip: async () => ({
      mime_type: "video/webm",
      data_base64: "dGlueS13ZWJt",
      width: 96,
      height: 96,
      fps: 25,
      frame_count: 75,
      duration_ms: 3000
    }),
    ingestClip: async (request) => {
      strictEqual(request.source.kind, "camera");
      strictEqual(request.quality_flags.mouth_landmarks_found, false);
      return roiRequest;
    }
  });

  strictEqual(result.roi.local_ref, roiRequest.roi.local_ref);
}

function runCameraRecognitionStatusTests() {
  strictEqual(
    createCameraRecognitionStatus({ cameraReady: true, modelReady: true, roiReady: false }).code,
    "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED"
  );
  strictEqual(
    createCameraRecognitionStatus({ cameraReady: true, modelReady: true, roiReady: false }).realRecognition,
    false
  );
}

function runCandidateMappingTests() {
  const mapped = mapDecodeCandidatesToOverlay(createTestCandidateResponse().candidates);
  strictEqual(mapped[0]?.text, "打开会议记录");
  strictEqual(mapped[0]?.source, "cnvsrc2025");
}

async function runRecognitionDecodeDispatchTests() {
  const events: AppEvent[] = [];
  await runDecodeWhenReady({
    cameraReady: true,
    modelReady: true,
    roiRequest: createTestRoiRequest(),
    decode: async () => ({
      response: createTestCandidateResponse(),
      candidates: createTestCandidateResponse().candidates,
      realDecode: true
    }),
    dispatch: (event) => events.push(event)
  });

  strictEqual(events[0]?.type, "ProcessingComplete");
}

async function runRecognitionSkipsFixtureDecodeTests() {
  const events: AppEvent[] = [];
  const fixtureResponse = createTestCandidateResponse();
  fixtureResponse.model.runtime_id = "cnvsrc2025-fixture-cpu";

  await runDecodeWhenReady({
    cameraReady: true,
    modelReady: true,
    roiRequest: createTestRoiRequest(),
    decode: async () => ({
      response: fixtureResponse,
      candidates: fixtureResponse.candidates,
      realDecode: false
    }),
    dispatch: (event) => events.push(event)
  });

  strictEqual(events.length, 0);
}

async function runRecognitionSkipsDecodeWithoutRoiTests() {
  let decodeCalls = 0;
  const events: AppEvent[] = [];
  await runDecodeWhenReady({
    cameraReady: true,
    modelReady: true,
    roiRequest: null,
    decode: async () => {
      decodeCalls += 1;
      return {
        response: createTestCandidateResponse(),
        candidates: createTestCandidateResponse().candidates,
        realDecode: true
      };
    },
    dispatch: (event) => events.push(event)
  });

  strictEqual(decodeCalls, 0);
  strictEqual(events.length, 0);
}

async function runLiveRoiDecodePipelineTests() {
  const events: AppEvent[] = [];
  let prepareCalls = 0;
  let decodeCalls = 0;
  const decoded = await runLiveRoiDecodeWhenReady({
    cameraReady: true,
    modelReady: true,
    prepareRoiRequest: async () => {
      prepareCalls += 1;
      return createTestRoiRequest();
    },
    decode: async () => {
      decodeCalls += 1;
      return {
        response: createTestCandidateResponse(),
        candidates: createTestCandidateResponse().candidates,
        realDecode: true
      };
    },
    dispatch: (event) => events.push(event)
  });

  strictEqual(prepareCalls, 1);
  strictEqual(decodeCalls, 1);
  strictEqual(decoded, true);
  strictEqual(events[0]?.type, "ProcessingComplete");

  const skipped = await runLiveRoiDecodeWhenReady({
    cameraReady: false,
    modelReady: true,
    prepareRoiRequest: async () => {
      throw new Error("prepare should not run without camera");
    },
    decode: async () => {
      throw new Error("decode should not run without camera");
    },
    dispatch: (event) => events.push(event)
  });

  strictEqual(skipped, false);
}

async function runCameraUnavailableTests() {
  const button = new MockButton();
  const status = new MockStatus();
  const video = new MockVideo();

  createCameraProbeController({
    button,
    status,
    video,
    getMediaDevices: () => undefined
  });

  await button.click();

  strictEqual(video.srcObject, null);
  strictEqual(video.hidden, true);
  strictEqual(button.disabled, false);
  strictEqual(
    status.textContent,
    "Camera access is unavailable in this WebView/browser context. Rebuild and run the Windows debug app to test the real permission prompt."
  );
  strictEqual(status.className, "camera-status camera-status-error");
}

async function runAllTests() {
  runTests();
  runModelStatusTests();
  runDevModeTests();
  runCameraAutoStartGateTests();
  runSidecarDecodeConfigTests();
  await runSidecarDecodeClientTests();
  await runSidecarRoiIngestClientTests();
  runRoiProducerQualityTests();
  await runPrepareCameraRoiRequestTests();
  runCameraRecognitionStatusTests();
  runCandidateMappingTests();
  await runRecognitionDecodeDispatchTests();
  await runRecognitionSkipsFixtureDecodeTests();
  await runRecognitionSkipsDecodeWithoutRoiTests();
  await runLiveRoiDecodePipelineTests();
  await runCameraProbeTests();
  await runCameraProbeStartMethodTests();
  await runCameraUnavailableTests();
}

await runAllTests();
