import { createCameraProbeController } from "../src/cameraProbe.ts";
import { shouldShowDevControls } from "../src/devMode.ts";
import { reduce } from "../src/hotkeyState.ts";
import { escapeModelStatusText, formatModelStatus } from "../src/modelStatus.ts";
import { renderCandidates } from "../src/render.ts";

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
  await runCameraProbeTests();
  await runCameraUnavailableTests();
}

await runAllTests();
