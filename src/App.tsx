import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	type DesktopCapabilities,
	unavailableCapabilityMessages,
} from "./capabilities";
import { ClearHistoryButton } from "./components/ClearHistoryButton";
import { ListItem } from "./components/ListItem";
import { useHistory } from "./hooks/useHistory";
import { logError } from "./log";
import { presentationForSession } from "./platform";
import { SettingsView } from "./SettingsView";
import { type ShortcutSettings, shortcutFromEvent } from "./shortcuts";
import "./App.css";

type PasteOutcome = "Pasted" | "CopiedForManualPaste";

function isInteractiveTarget(target: EventTarget | null) {
	return (
		target instanceof Element &&
		target.closest(
			"button, input, select, textarea, [contenteditable='true'], [role='button']",
		) !== null
	);
}

const MenuSeparator = () => (
	<div className="menu__separator h-px my-[4px] mx-0 bg-[rgba(235,235,245,0.18)]" />
);

export function App() {
	const {
		items: clipboard,
		selectedHash,
		setSelectedHash,
		error: historyError,
		setError: setHistoryError,
		refresh: refreshHistory,
		deleteSelected,
	} = useHistory();
	const selectedItem = clipboard.findIndex(
		(item) => item.hash === selectedHash,
	);
	const [shortcuts, setShortcuts] = useState<ShortcutSettings | null>(null);
	const [capabilities, setCapabilities] = useState<DesktopCapabilities | null>(
		null,
	);
	const [manualPasteCopied, setManualPasteCopied] = useState(false);
	const platformPresentation = presentationForSession(capabilities?.session);

	const historyRef = useRef<HTMLDivElement>(null);
	const pasteRequest = useRef(0);

	const invalidatePasteRequest = useCallback(() => {
		pasteRequest.current += 1;
		setManualPasteCopied(false);
	}, []);

	const hide = useCallback(() => {
		invalidatePasteRequest();
		void invoke("close").catch((error) => {
			logError("Failed to close picker", error);
			setHistoryError(String(error));
		});
	}, [invalidatePasteRequest, setHistoryError]);

	const loadCapabilities = useCallback(async () => {
		try {
			const capabilities =
				await invoke<DesktopCapabilities>("get_capabilities");
			setCapabilities(capabilities);
		} catch (error) {
			logError("Failed to get desktop capabilities", error);
		}
	}, []);

	const unavailableCapabilities = useMemo(
		() => (capabilities ? unavailableCapabilityMessages(capabilities) : []),
		[capabilities],
	);

	const clearHistory = useCallback(async () => {
		invalidatePasteRequest();

		try {
			await invoke("clear");
			await refreshHistory();
		} catch (error) {
			logError("Failed to clear clipboard history", error);
			setHistoryError(String(error));
		}
	}, [invalidatePasteRequest, refreshHistory, setHistoryError]);

	const showSettings = useCallback(() => {
		void invoke("show_settings").catch((error) => {
			logError("Failed to show settings", error);
			setHistoryError(String(error));
		});
	}, [setHistoryError]);

	const quitApplication = useCallback(() => {
		void invoke("quit").catch((error) =>
			logError("Failed to quit Klipo", error),
		);
	}, []);

	const pasteFromSelection = useCallback(
		async (hash: string) => {
			invalidatePasteRequest();
			const request = pasteRequest.current;
			try {
				const outcome = await invoke<PasteOutcome>("paste", { hash });
				if (request !== pasteRequest.current) return;
				if (outcome === "CopiedForManualPaste") setManualPasteCopied(true);
			} catch (reason) {
				logError("Failed to paste from selection", reason);
				if (request === pasteRequest.current) {
					setHistoryError(
						reason instanceof Error ? reason.message : String(reason),
					);
				}
			}
		},
		[invalidatePasteRequest, setHistoryError],
	);

	const deleteItem = useCallback(
		async (hash: string) => {
			invalidatePasteRequest();
			try {
				const outcome = await deleteSelected(hash);
				if (outcome === "DeletedWithClipboardWarning") {
					setHistoryError(
						"Item deleted, but the clipboard could not be updated.",
					);
				}
			} catch (reason) {
				logError("Failed to delete clipboard item", reason);
				setHistoryError(String(reason));
			}
		},
		[invalidatePasteRequest, deleteSelected, setHistoryError],
	);

	const handleKeyDown = useCallback(
		(event: KeyboardEvent) => {
			invalidatePasteRequest();

			if (event.key === "Escape") {
				event.preventDefault();
				hide();
				return;
			}

			if (isInteractiveTarget(event.target)) return;

			if (!shortcuts) return;
			const isValidItem = (itemIdx: number) =>
				itemIdx >= 0 && itemIdx < clipboard.length;

			let newSelectedItem = selectedItem;

			const pressedShortcut = shortcutFromEvent(event);
			switch (pressedShortcut) {
				case shortcuts.moveSelectionUp:
					event.preventDefault();

					newSelectedItem =
						selectedItem > 0 ? selectedItem - 1 : clipboard.length - 1;

					break;
				case shortcuts.moveSelectionDown:
					event.preventDefault();

					newSelectedItem =
						selectedItem >= 0 && selectedItem < clipboard.length - 1
							? selectedItem + 1
							: 0;

					break;
				case shortcuts.pasteSelectedItem: {
					event.preventDefault();

					if (isValidItem(selectedItem)) {
						pasteFromSelection(clipboard[selectedItem].hash);
					}

					return;
				}
				case shortcuts.deleteSelectedItem:
					event.preventDefault();

					if (isValidItem(selectedItem)) {
						deleteItem(clipboard[selectedItem].hash);
					}

					break;
				default:
					break;
			}

			if (newSelectedItem >= 0 && newSelectedItem !== selectedItem) {
				historyRef.current?.children[newSelectedItem]?.scrollIntoView({
					block: "nearest",
				});
			}

			if (newSelectedItem !== selectedItem) {
				setSelectedHash(clipboard[newSelectedItem]?.hash ?? null);
			}
		},
		[
			clipboard,
			selectedItem,
			setSelectedHash,
			pasteFromSelection,
			hide,
			deleteItem,
			shortcuts,
			invalidatePasteRequest,
		],
	);

	const handleBlur = useCallback(() => {
		setSelectedHash(null);
		setManualPasteCopied(false);
	}, [setSelectedHash]);

	const handleFocus = useCallback(() => {
		loadCapabilities();
		invoke<ShortcutSettings>("get_shortcuts")
			.then(setShortcuts)
			.catch((error) => logError("Failed to get keyboard shortcuts", error));
	}, [loadCapabilities]);

	useEffect(() => {
		handleFocus();
	}, [handleFocus]);

	useEffect(() => {
		window.addEventListener("keydown", handleKeyDown);
		window.addEventListener("focus", handleFocus);
		window.addEventListener("blur", handleBlur);

		return () => {
			window.removeEventListener("keydown", handleKeyDown);
			window.removeEventListener("focus", handleFocus);
			window.removeEventListener("blur", handleBlur);
		};
	}, [handleKeyDown, handleBlur, handleFocus]);

	return (
		<div className={`menu ${platformPresentation.className}`}>
			<div className="menu__content">
				<div className="flex justify-between items-center mx-2">
					<div className="flex justify-left items-center">
						<span className="text-base font-bold">Klipo</span>
					</div>

					<div className="flex items-center gap-1">
						<button
							type="button"
							className="menu__control"
							onClick={showSettings}
						>
							Settings
						</button>
						<button
							type="button"
							className="menu__control"
							onClick={quitApplication}
						>
							Quit
						</button>
						<ClearHistoryButton onClick={clearHistory} />
					</div>
				</div>
				{manualPasteCopied && (
					<p role="status" aria-live="polite" className="mx-2 text-sm">
						Copied
					</p>
				)}

				{historyError && (
					<p role="alert" className="mx-2 text-sm">
						{historyError}
					</p>
				)}

				<MenuSeparator />

				{unavailableCapabilities.length > 0 && (
					<div className="menu__capabilities" role="status">
						{unavailableCapabilities.map((message) => (
							<p key={message}>{message}</p>
						))}
					</div>
				)}

				<div
					ref={historyRef}
					className={`menu__history${selectedHash === null ? "" : " has-keyboard-selection"}`}
					onPointerMove={() => setSelectedHash(null)}
				>
					{clipboard.map((item) => (
						<ListItem
							key={item.hash}
							label={item.text || `Image ${item.hash.slice(0, 8)}`}
							preview={item.preview}
							onClick={() => pasteFromSelection(item.hash)}
							active={item.hash === selectedHash}
						/>
					))}
				</div>
			</div>
		</div>
	);
}

export default function Root() {
	return getCurrentWindow().label === "settings" ? <SettingsView /> : <App />;
}
