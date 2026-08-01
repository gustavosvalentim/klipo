import { invoke } from "@tauri-apps/api/core";

export function logError(context: string, error: unknown) {
	void invoke("log_frontend_error", {
		context,
		error: error instanceof Error ? error.message : String(error),
	}).catch(() => undefined);
}
