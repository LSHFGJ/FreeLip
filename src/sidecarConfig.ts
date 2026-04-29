import debugConfig from "../config/freelip.debug.json" with { type: "json" };

type FreeLipDebugConfig = {
	sidecar?: {
		host?: string;
		port?: number;
		token?: string;
	};
};

const sidecarConfig = (debugConfig as FreeLipDebugConfig).sidecar ?? {};

export const SIDECAR_HOST = sidecarConfig.host ?? "127.0.0.1";
export const SIDECAR_PORT = sidecarConfig.port ?? 18765;
export const DEBUG_SIDECAR_TOKEN =
	sidecarConfig.token ?? "debug-local-token-change-before-sharing";
export const SIDECAR_STATUS_URL = `http://${SIDECAR_HOST}:${SIDECAR_PORT}/model/status`;
