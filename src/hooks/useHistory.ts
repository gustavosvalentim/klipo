import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { logError } from "../log";

export type HistorySummary = {
	hash: string;
	text: string;
	preview?: string;
};

export function useHistory() {
	const [items, setItems] = useState<HistorySummary[]>([]);
	const [selectedHash, setSelectedHash] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const requestId = useRef(0);
	const mounted = useRef(false);
	const deletionInProgress = useRef(false);

	const refresh = useCallback(async () => {
		if (deletionInProgress.current) return;
		const request = ++requestId.current;
		try {
			const history = await invoke<HistorySummary[]>("fetch_clipboard");
			if (!mounted.current || request !== requestId.current) return;
			setItems(history);
			setSelectedHash((previous) =>
				previous && history.some((item) => item.hash === previous)
					? previous
					: null,
			);
			setError(null);
		} catch (reason) {
			if (!mounted.current || request !== requestId.current) return;
			logError("Failed to get clipboard history", reason);
			setError("Could not load clipboard history.");
		}
	}, []);

	useEffect(() => {
		mounted.current = true;
		void refresh();
		const onFocus = () => void refresh();
		window.addEventListener("focus", onFocus);
		let unlisten: (() => void) | undefined;
		void listen("clipboard-changed", async () => {
			try {
				if (await getCurrentWindow().isVisible()) await refresh();
			} catch (reason) {
				logError("Failed to check picker visibility", reason);
			}
		})
			.then((release) => {
				if (mounted.current) unlisten = release;
				else release();
			})
			.catch((reason) => logError("Failed to watch clipboard history", reason));
		return () => {
			mounted.current = false;
			requestId.current += 1;
			window.removeEventListener("focus", onFocus);
			unlisten?.();
		};
	}, [refresh]);

	const deleteSelected = useCallback(
		async (hash: string) => {
			if (deletionInProgress.current)
				throw new Error("A deletion is already in progress");
			deletionInProgress.current = true;
			requestId.current += 1;
			const selectedAtStart = selectedHash;
			const index = items.findIndex((item) => item.hash === hash);
			const predecessor = index > 0 ? items[index - 1].hash : null;
			try {
				const outcome = await invoke<"Deleted" | "DeletedWithClipboardWarning">(
					"delete_item",
					{ hash },
				);
				if (selectedAtStart === hash) {
					setSelectedHash((current) =>
						current === hash ? predecessor : current,
					);
				}
				deletionInProgress.current = false;
				await refresh();
				return outcome;
			} finally {
				deletionInProgress.current = false;
			}
		},
		[items, selectedHash, refresh],
	);

	return {
		items,
		selectedHash,
		setSelectedHash,
		error,
		setError,
		refresh,
		deleteSelected,
	};
}
