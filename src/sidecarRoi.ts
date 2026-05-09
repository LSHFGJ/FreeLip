import type { RoiQualityFlags, RoiRequest } from "./sidecarDecode.ts";

export type RoiClipPayload = {
  mime_type: "video/webm" | "video/mp4" | "application/octet-stream";
  data_base64: string;
  width: number;
  height: number;
  fps: number;
  frame_count: number;
  duration_ms: number;
};

export type RoiClipIngestRequest = {
  schema_version: "1.0.0";
  request_id: string;
  session_id: string;
  source: RoiRequest["source"];
  clip: RoiClipPayload;
  quality_flags: RoiQualityFlags;
  requested_at_ms: number;
};

export type RoiClipIngestResponse = {
  schema_version: "1.0.0";
  roi_request: RoiRequest;
};

export type RoiClipClientOptions = {
  url: string;
  token: string;
  fetchJson?: (url: string, init: RequestInit) => Promise<unknown>;
};

export type LiveRoiReadiness = {
  hasCameraStream: boolean;
  hasVideoElement: boolean;
  mediaRecorderAvailable: boolean;
  canvasCaptureAvailable: boolean;
};

export type PrepareCameraRoiRequestOptions = {
  sessionId: string;
  sourceStartedAtMs: number;
  requestedAtMs?: number;
  recordClip: () => Promise<RoiClipPayload>;
  ingestClip: (request: RoiClipIngestRequest) => Promise<RoiRequest>;
};

export type RecordVideoElementClipOptions = {
  video: HTMLVideoElement;
  durationMs?: number;
  fps?: number;
  targetWidth?: number;
  targetHeight?: number;
  mimeType?: "video/webm" | "video/mp4";
  mediaRecorderCtor?: typeof MediaRecorder;
  createCanvas?: () => HTMLCanvasElement;
  setIntervalFn?: typeof setInterval;
  clearIntervalFn?: typeof clearInterval;
  setTimeoutFn?: typeof setTimeout;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isRoiRequest(value: unknown): value is RoiRequest {
  if (!isRecord(value)) return false;
  if (value.schema_version !== "1.0.0") return false;
  if (typeof value.request_id !== "string") return false;
  if (typeof value.session_id !== "string") return false;
  if (!isRecord(value.roi) || typeof value.roi.local_ref !== "string") return false;
  return value.roi.local_ref.startsWith("local://path/") || value.roi.local_ref.startsWith("local://file/");
}

function isRoiClipIngestResponse(value: unknown): value is RoiClipIngestResponse {
  return isRecord(value) && value.schema_version === "1.0.0" && isRoiRequest(value.roi_request);
}

async function defaultFetchJson(url: string, init: RequestInit): Promise<unknown> {
  const response = await fetch(url, init);
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(`Sidecar ROI ingest failed with HTTP_${response.status}`);
  }
  return payload;
}

export function createDefaultRoiQualityFlags(): RoiQualityFlags {
  return {
    schema_version: "1.0.0",
    face_found: false,
    mouth_landmarks_found: false,
    crop_bounds_valid: false,
    blur_ok: true,
    brightness_ok: true,
    pose_ok: true,
    occlusion_ok: true,
    landmark_confidence: 0,
    rejection_reasons: ["face_not_found", "mouth_landmarks_missing", "crop_bounds_invalid"]
  };
}

export function canCaptureLiveRoi(input: LiveRoiReadiness): boolean {
  return input.hasCameraStream && input.hasVideoElement && input.mediaRecorderAvailable && input.canvasCaptureAvailable;
}

export function hasCanvasCaptureSupport(createCanvas: () => HTMLCanvasElement = () => document.createElement("canvas")): boolean {
  try {
    const canvas = createCanvas();
    return typeof canvas.captureStream === "function" && canvas.getContext("2d") !== null;
  } catch {
    return false;
  }
}

export async function ingestRoiClipWithSidecar(
  request: RoiClipIngestRequest,
  options: RoiClipClientOptions
): Promise<RoiRequest> {
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

  if (!isRoiClipIngestResponse(payload)) {
    throw new Error("Sidecar ROI ingest response did not include an adapter-compatible RoiRequest");
  }
  return payload.roi_request;
}

export async function prepareCameraRoiRequest({
  sessionId,
  sourceStartedAtMs,
  requestedAtMs = Date.now(),
  recordClip,
  ingestClip
}: PrepareCameraRoiRequestOptions): Promise<RoiRequest> {
  const clip = await recordClip();
  const request: RoiClipIngestRequest = {
    schema_version: "1.0.0",
    request_id: `roi-${sessionId}-${requestedAtMs}`,
    session_id: sessionId,
    source: {
      kind: "camera",
      started_at_ms: sourceStartedAtMs
    },
    clip,
    quality_flags: createDefaultRoiQualityFlags(),
    requested_at_ms: requestedAtMs
  };
  return ingestClip(request);
}

export async function recordVideoElementClip({
  video,
  durationMs = 3000,
  fps = 25,
  targetWidth = 96,
  targetHeight = 96,
  mimeType = "video/webm",
  mediaRecorderCtor = globalThis.MediaRecorder,
  createCanvas = () => document.createElement("canvas"),
  setIntervalFn = setInterval,
  clearIntervalFn = clearInterval,
  setTimeoutFn = setTimeout
}: RecordVideoElementClipOptions): Promise<RoiClipPayload> {
  if (!mediaRecorderCtor) {
    throw new Error("MediaRecorder is unavailable in this WebView");
  }
  const canvas = createCanvas();
  canvas.width = targetWidth;
  canvas.height = targetHeight;
  const context = canvas.getContext("2d");
  if (!context) {
    throw new Error("Canvas 2D context is unavailable for ROI capture");
  }
  if (typeof canvas.captureStream !== "function") {
    throw new Error("Canvas captureStream is unavailable for ROI capture");
  }

  const stream = canvas.captureStream(fps);
  const chunks: Blob[] = [];
  const recorder = new mediaRecorderCtor(stream, { mimeType });
  const frameIntervalMs = Math.max(1, Math.floor(1000 / fps));
  const drawTimer = setIntervalFn(() => {
    context.drawImage(video, 0, 0, targetWidth, targetHeight);
  }, frameIntervalMs);

  return new Promise<RoiClipPayload>((resolve, reject) => {
    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) {
        chunks.push(event.data);
      }
    };
    recorder.onerror = () => {
      clearIntervalFn(drawTimer);
      reject(new Error("MediaRecorder failed during ROI capture"));
    };
    recorder.onstop = () => {
      clearIntervalFn(drawTimer);
      void (async () => {
        const blob = new Blob(chunks, { type: mimeType });
        const dataBase64 = await blobToBase64(blob);
        resolve({
          mime_type: mimeType,
          data_base64: dataBase64,
          width: targetWidth,
          height: targetHeight,
          fps,
          frame_count: Math.max(1, Math.round((durationMs / 1000) * fps)),
          duration_ms: durationMs
        });
      })().catch(reject);
    };

    context.drawImage(video, 0, 0, targetWidth, targetHeight);
    recorder.start();
    setTimeoutFn(() => recorder.stop(), durationMs);
  });
}

async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}
