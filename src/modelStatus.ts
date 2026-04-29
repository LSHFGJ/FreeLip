export type SidecarModelStatus = {
  backend?: unknown;
  status?: unknown;
  fixture_mode?: unknown;
  fallback_active?: unknown;
  ready?: unknown;
  model_id?: unknown;
  runtime_id?: unknown;
  error_code?: unknown;
  readiness_status?: unknown;
  exit_code?: unknown;
  message?: unknown;
};

export type ModelStatusTone = "pending" | "ready" | "warning" | "error";

export type FormattedModelStatus = {
  text: string;
  tone: ModelStatusTone;
  realModelReady: boolean;
};

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function isFixtureStatus(status: SidecarModelStatus): boolean {
  const backend = stringValue(status.backend)?.toLowerCase();
  const runtimeId = stringValue(status.runtime_id)?.toLowerCase();
  const statusCode = stringValue(status.status)?.toUpperCase();
  return (
    backend === "fixture" ||
    status.fixture_mode === true ||
    status.fallback_active === true ||
    statusCode === "FIXTURE_MODE" ||
    runtimeId?.includes("fixture") === true
  );
}

export function formatModelStatus(status: SidecarModelStatus | null | undefined): FormattedModelStatus {
  if (!status) {
    return {
      text: "Model status: checking local sidecar on 127.0.0.1:8765...",
      tone: "pending",
      realModelReady: false
    };
  }

  const modelId = stringValue(status.model_id) ?? "cnvsrc2025";
  const runtimeId = stringValue(status.runtime_id) ?? "runtime unavailable";
  const statusCode = stringValue(status.status) ?? stringValue(status.error_code) ?? "MODEL_UNAVAILABLE";
  const errorCode = stringValue(status.error_code) ?? statusCode;
  const readinessStatus = stringValue(status.readiness_status);
  const message = stringValue(status.message);

  if (isFixtureStatus(status)) {
    const readiness = readinessStatus ? ` Readiness gate: ${readinessStatus}.` : "";
    return {
      text: `Fixture/mock mode: not real model inference. ${modelId} is using ${runtimeId}.${readiness}`,
      tone: "warning",
      realModelReady: false
    };
  }

  if (status.ready === true && errorCode !== "MODEL_UNAVAILABLE") {
    return {
      text: `Real model ready: ${modelId} via ${runtimeId}.`,
      tone: "ready",
      realModelReady: true
    };
  }

  const details = message ? ` ${message}` : "";
  return {
    text: `Real model not ready: ${modelId} reports ${statusCode} via ${runtimeId}.${details}`,
    tone: "error",
    realModelReady: false
  };
}
