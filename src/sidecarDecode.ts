export type RoiQualityFlags = {
  schema_version: "1.0.0";
  face_found: boolean;
  mouth_landmarks_found: boolean;
  crop_bounds_valid: boolean;
  blur_ok: boolean;
  brightness_ok: boolean;
  pose_ok: boolean;
  occlusion_ok: boolean;
  landmark_confidence: number;
  rejection_reasons: string[];
};

export type RoiRequest = {
  schema_version: "1.0.0";
  request_id: string;
  session_id: string;
  source: {
    kind: "camera" | "public_video" | "fixture";
    device_id_hash?: string;
    started_at_ms: number;
  };
  roi: {
    local_ref: string;
    format: "grayscale_u8" | "rgb_u8" | "tensor_f32_nchw";
    width: number;
    height: number;
    fps: number;
    frame_count: number;
    duration_ms: number;
  };
  quality_flags: RoiQualityFlags;
  requested_at_ms: number;
};

export type CandidateResponseCandidate = {
  schema_version: "1.0.0";
  rank: number;
  text: string;
  score: number;
  source: "vsr" | "cnvsrc2025" | "dictionary" | "llm_rerank" | "manual";
  is_auto_insert_eligible: boolean;
};

export type CandidateResponse = {
  schema_version: "1.0.0";
  request_id: string;
  session_id: string;
  model: {
    model_id: string;
    runtime_id: string;
    device: "cpu" | "cuda" | "directml";
  };
  candidates: CandidateResponseCandidate[];
  quality_flags: RoiQualityFlags;
  timing_ms: {
    roi_received_to_first_candidate: number;
    roi_received_to_final: number;
  };
  created_at_ms: number;
};

export type DecodeClientOptions = {
  url: string;
  token: string;
  fetchJson?: (url: string, init: RequestInit) => Promise<unknown>;
};

export type DecodeResult = {
  response: CandidateResponse;
  candidates: CandidateResponseCandidate[];
  realDecode: boolean;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isCandidateResponse(value: unknown): value is CandidateResponse {
  if (!isRecord(value)) return false;
  if (value.schema_version !== "1.0.0") return false;
  if (typeof value.request_id !== "string") return false;
  if (typeof value.session_id !== "string") return false;
  if (!Array.isArray(value.candidates) || value.candidates.length === 0) return false;
  if (!isRecord(value.model) || typeof value.model.runtime_id !== "string") return false;
  return value.candidates.every((candidate) => {
    if (!isRecord(candidate)) return false;
    return typeof candidate.text === "string" && typeof candidate.source === "string";
  });
}

async function defaultFetchJson(url: string, init: RequestInit): Promise<unknown> {
  const response = await fetch(url, init);
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(`Sidecar decode failed with HTTP_${response.status}`);
  }
  return payload;
}

function mapCandidateResponse(payload: unknown): DecodeResult {
  if (!isCandidateResponse(payload)) {
    throw new Error("Sidecar decode response did not match the candidate contract");
  }

  const runtimeId = payload.model.runtime_id.toLowerCase();
  const realDecode = !runtimeId.includes("fixture");
  return {
    response: payload,
    candidates: payload.candidates,
    realDecode
  };
}

export async function decodeRoiWithSidecar(
  request: RoiRequest,
  options: DecodeClientOptions
): Promise<DecodeResult> {
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
