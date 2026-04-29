import "./styles.css";
import { createCameraProbeController } from "./cameraProbe.ts";
import { readDevControlsEnabled } from "./devMode.ts";
import type { AppEvent, AppState, OverlayCandidate } from "./hotkeyState.ts";
import { reduce } from "./hotkeyState.ts";
import type {
	FormattedModelStatus,
	SidecarModelStatus,
} from "./modelStatus.ts";
import { escapeModelStatusText, formatModelStatus } from "./modelStatus.ts";
import { renderCandidates } from "./render.ts";

let state: AppState = { type: "Idle", chord: "Ctrl+Alt+Space" };
let modelStatus: FormattedModelStatus = formatModelStatus(null);

const SIDECAR_STATUS_URL = "http://127.0.0.1:8765/model/status";
const DEBUG_SIDECAR_TOKEN = "debug-local-token-change-before-sharing";

const app = document.querySelector<HTMLDivElement>("#app");
const devControlsEnabled = readDevControlsEnabled();

function render() {
	if (!app) return;

	let content = `
    <section class="shell">
      <p class="eyebrow">FreeLip internal MVP</p>
      <h1>Hotkey Overlay</h1>
      <div class="state-indicator status-${state.type.toLowerCase()}">
        Status: <strong>${state.type}</strong>
      </div>
      <section id="model-status" class="model-status model-status-${modelStatus.tone}" aria-live="polite">
        <p class="eyebrow">CNVSRC runtime</p>
        <p>${escapeModelStatusText(modelStatus.text)}</p>
      </section>
  `;

	if (state.type === "Idle") {
		content += `<p class="instructions">Press <code>${state.chord}</code> to start recording.</p>`;
	} else if (state.type === "CollisionRemapRequired") {
		content += `
      <div class="alert error">
        <p><strong>Hotkey Collision:</strong> <code>${state.defaultChord}</code> is already in use.</p>
        <p>You must configure a replacement hotkey before capture can start.</p>
        <button id="btn-remap">Configure replacement (Ctrl+Alt+L)</button>
      </div>
    `;
	} else if (state.type === "Recording") {
		content += `
      <div class="recording-pulse"></div>
      <p class="instructions">Listening... Press <code>${state.chord}</code> again to stop.</p>
    `;
	} else if (state.type === "Processing") {
		content += `
      <p class="instructions">Processing visual speech... Please wait.</p>
      <div class="loader"></div>
    `;
	} else if (state.type === "ShowingCandidates") {
		if (state.lowQuality) {
			content += `
        <div class="alert warning">
          <p><strong>Low Quality Visuals:</strong> Recognition might be inaccurate.</p>
        </div>
      `;
		}

		if (!state.autoInsertThresholdMet) {
			content += `
        <div class="alert info">
          <p><strong>Auto-insert disabled:</strong> Confidence threshold not met. Manual selection required.</p>
        </div>
      `;
		}

		content += `
      <div class="candidates-overlay">
        <h3>Candidates</h3>
        <ol class="candidates-list"></ol>
        <p class="overlay-help">Press <code>1-5</code> to select, <code>Esc</code> to cancel.</p>
      </div>
    `;
	}

	content += `
      <section class="camera-probe" aria-labelledby="camera-probe-title">
        <p class="eyebrow">Windows camera check</p>
        <h2 id="camera-probe-title">Camera Probe</h2>
        <p class="instructions">
          Request camera access and show a local preview in this WebView. This does not run ROI cropping, VSR, or model inference.
        </p>
        <button id="camera-probe-request" type="button">Request camera preview</button>
        <p id="camera-probe-status" class="camera-status camera-status-idle">
          Camera not requested yet. Run this from the Windows debug app to verify the real permission prompt.
        </p>
        <video id="camera-probe-preview" class="camera-preview" autoplay muted playsinline hidden></video>
      </section>

      ${
				devControlsEnabled
					? `
        <div class="dev-controls" aria-label="Developer-only fixture controls">
          <p class="dev-controls-warning">Developer fixture controls are enabled. Candidate buttons below do not run real model inference.</p>
          <button id="dev-trigger-recording">Trigger: Recording</button>
          <button id="dev-trigger-collision">Trigger: Collision</button>
          <button id="dev-trigger-candidates-high">Trigger: Candidates (High Quality, Auto)</button>
          <button id="dev-trigger-candidates-low">Trigger: Candidates (Low Quality, Manual)</button>
        </div>
      `
					: ""
			}
    </section>
  `;

	app.innerHTML = content;

	if (state.type === "ShowingCandidates") {
		const list = app.querySelector(".candidates-list");
		if (list) {
			renderCandidates(list, state.candidates);
		}
	}

	attachEvents();
}

function dispatch(event: AppEvent) {
	const result = reduce(state, event);
	state = result.state;

	if (result.action.type === "InsertCandidate") {
		showToast(`Inserted: ${result.action.candidate.text}`);
	} else if (result.action.type === "Cancel") {
		showToast("Operation cancelled.");
	}

	render();
}

function attachEvents() {
	const cameraButton = document.querySelector<HTMLButtonElement>(
		"#camera-probe-request",
	);
	const cameraStatus = document.querySelector<HTMLParagraphElement>(
		"#camera-probe-status",
	);
	const cameraVideo = document.querySelector<HTMLVideoElement>(
		"#camera-probe-preview",
	);
	if (cameraButton && cameraStatus && cameraVideo) {
		createCameraProbeController({
			button: cameraButton,
			status: cameraStatus,
			video: cameraVideo,
			getMediaDevices: () => navigator.mediaDevices,
		});
	}

	const remapBtn = document.getElementById("btn-remap");
	if (remapBtn) {
		remapBtn.addEventListener("click", () =>
			dispatch({ type: "Remapped", newChord: "Ctrl+Alt+L" }),
		);
	}

	const items = document.querySelectorAll(".candidate-item");
	items.forEach((item) => {
		item.addEventListener("click", (e) => {
			const idxStr = (e.currentTarget as HTMLElement).getAttribute(
				"data-index",
			);
			if (idxStr !== null) {
				dispatch({ type: "MouseSelected", index: parseInt(idxStr, 10) });
			}
		});
	});

	const devRec = document.getElementById("dev-trigger-recording");
	if (devRec)
		devRec.addEventListener("click", () => {
			dispatch({ type: "HotkeyPressed" });
		});

	const devCol = document.getElementById("dev-trigger-collision");
	if (devCol)
		devCol.addEventListener("click", () => {
			dispatch({ type: "CollisionDetected" });
		});

	if (devControlsEnabled) {
		const testCandidates: OverlayCandidate[] = [
			{ text: "Fixture Option A", source: "fixture" },
			{ text: "Fixture Option B", source: "fixture" },
			{ text: "Fixture Option C", source: "fixture" },
			{ text: "Fixture Option D", source: "fixture" },
			{ text: "Fixture Option E", source: "fixture" },
		];

		const devCandHigh = document.getElementById("dev-trigger-candidates-high");
		if (devCandHigh)
			devCandHigh.addEventListener("click", () => {
				dispatch({
					type: "ProcessingComplete",
					candidates: testCandidates,
					lowQuality: false,
					autoInsertThresholdMet: true,
				});
			});

		const devCandLow = document.getElementById("dev-trigger-candidates-low");
		if (devCandLow)
			devCandLow.addEventListener("click", () => {
				dispatch({
					type: "ProcessingComplete",
					candidates: testCandidates,
					lowQuality: true,
					autoInsertThresholdMet: false,
				});
			});
	}
}

function showToast(msg: string) {
	const toast = document.createElement("div");
	toast.className = "toast";
	toast.textContent = msg;
	document.body.appendChild(toast);
	setTimeout(() => {
		toast.style.opacity = "0";
		setTimeout(() => toast.remove(), 300);
	}, 3000);
}

async function refreshModelStatus() {
	try {
		const response = await fetch(SIDECAR_STATUS_URL, {
			cache: "no-store",
			headers: {
				Authorization: `Bearer ${DEBUG_SIDECAR_TOKEN}`,
				"X-FreeLip-Token": DEBUG_SIDECAR_TOKEN,
			},
		});
		const payload = (await response.json()) as SidecarModelStatus;
		if (!response.ok) {
			modelStatus = formatModelStatus({
				...payload,
				ready: false,
				status: payload.error_code ?? `HTTP_${response.status}`,
			});
		} else {
			modelStatus = formatModelStatus(payload);
		}
	} catch (error) {
		modelStatus = formatModelStatus({
			ready: false,
			backend: "cnvsrc2025",
			status: "SIDECAR_UNREACHABLE",
			error_code: "SIDECAR_UNREACHABLE",
			model_id: "cnvsrc2025",
			runtime_id: "local-sidecar",
			message:
				error instanceof Error
					? error.message
					: "local sidecar status request failed",
		});
	}
	render();
}

document.addEventListener("keydown", (e) => {
	// Allow Ctrl+Alt+Space or Ctrl+Alt+L depending on state chord
	const isDefault = e.ctrlKey && e.altKey && e.code === "Space";
	const isRemapped = e.ctrlKey && e.altKey && e.code === "KeyL";

	if (isDefault || isRemapped) {
		// Only accept if it matches the current expected chord
		if (
			(state.type === "Idle" || state.type === "Recording") &&
			state.chord === "Ctrl+Alt+Space" &&
			!isDefault
		)
			return;
		if (
			(state.type === "Idle" || state.type === "Recording") &&
			state.chord === "Ctrl+Alt+L" &&
			!isRemapped
		)
			return;

		e.preventDefault();
		if (state.type === "Idle") {
			dispatch({ type: "HotkeyPressed" });
		} else if (state.type === "Recording") {
			dispatch({ type: "RecordingStopped" });
		}
		return;
	}

	if (state.type === "ShowingCandidates") {
		if (e.key >= "1" && e.key <= "5") {
			e.preventDefault();
			dispatch({ type: "NumberKeyPressed", index: parseInt(e.key, 10) });
			return;
		}
	}

	if (e.key === "Escape") {
		if (
			state.type === "Recording" ||
			state.type === "Processing" ||
			state.type === "ShowingCandidates"
		) {
			dispatch({ type: "EscapePressed" });
		}
	}
});

render();
void refreshModelStatus();
setInterval(() => {
	void refreshModelStatus();
}, 5000);
