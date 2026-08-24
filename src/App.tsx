import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	type DesktopCapabilities,
	unavailableCapabilityMessages,
} from "./capabilities";
import { ClearHistoryButton } from "./components/ClearHistoryButton";
import { ListItem } from "./components/ListItem";
import { logError } from "./log";
import { getPlatformPresentation, shortcutLabel } from "./platform";
import "./App.css";

type ClipboardItem = {
	hash: string;
	text: string;
	preview?: string;
};

type Clipboard = ClipboardItem[];

type PasteOutcome = "Pasted" | "CopiedForManualPaste" | "ClipboardWriteFailed";

type ShortcutSettings = {
	version: number;
	openKlipo: string;
	moveSelectionUp: string;
	moveSelectionDown: string;
	pasteSelectedItem: string;
	deleteSelectedItem: string;
};

type ShortcutField = Exclude<keyof ShortcutSettings, "version">;

const shortcutFields: Array<[ShortcutField, string]> = [
	["openKlipo", "Open Klipo"],
	["moveSelectionUp", "Move selection up"],
	["moveSelectionDown", "Move selection down"],
	["pasteSelectedItem", "Paste selected item"],
	["deleteSelectedItem", "Delete selected item"],
];

const modifierKeys = new Set(["Meta", "Control", "Alt", "Shift"]);

function shortcutFromEvent(event: KeyboardEvent) {
	if (modifierKeys.has(event.key) || event.key === "Escape") return null;
	const modifiers = [
		event.metaKey && "SUPER",
		event.ctrlKey && "CTRL",
		event.altKey && "ALT",
		event.shiftKey && "SHIFT",
	].filter(Boolean);
	return [...modifiers, event.code].join("+");
}

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
	const [clipboard, setClipboard] = useState<Clipboard>([]);
	const [selectedItem, setSelectedItem] = useState<number | null>(null);
	const [shortcuts, setShortcuts] = useState<ShortcutSettings | null>(null);
	const [capabilities, setCapabilities] = useState<DesktopCapabilities | null>(
		null,
	);
	const [manualPasteCopied, setManualPasteCopied] = useState(false);
	const platformPresentation = getPlatformPresentation();

	const historyRef = useRef<HTMLDivElement>(null);
	const pasteRequest = useRef(0);

	const invalidatePasteRequest = useCallback(() => {
		pasteRequest.current += 1;
		setManualPasteCopied(false);
	}, []);

	const hide = useCallback(() => {
		invalidatePasteRequest();
		void invoke("close").catch((error) =>
			logError("Failed to close picker", error),
		);
	}, [invalidatePasteRequest]);

	const fetchClipboardHistory = useCallback(async () => {
		try {
			const clipboard = await invoke<ClipboardItem[]>("fetch_clipboard");
			setClipboard(clipboard);
		} catch (error) {
			logError("Failed to get clipboard history", error);
		}
	}, []);

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
		} catch (error) {
			logError("Failed to clear clipboard history", error);
		}
	}, [invalidatePasteRequest]);

	const showSettings = useCallback(() => {
		void invoke("show_settings").catch((error) =>
			logError("Failed to show settings", error),
		);
	}, []);

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

				if (
					request === pasteRequest.current &&
					outcome === "CopiedForManualPaste"
				) {
					const picker = getCurrentWindow();
					const showedPicker = await picker
						.show()
						.then(() => true)
						.catch((error) => {
							logError("Failed to show picker for manual paste", error);
							return false;
						});
					const focusedPicker =
						request === pasteRequest.current
							? await picker
									.setFocus()
									.then(() => true)
									.catch((error) => {
										logError("Failed to focus picker for manual paste", error);
										return false;
									})
							: false;

					if (
						request === pasteRequest.current &&
						showedPicker &&
						focusedPicker
					) {
						setManualPasteCopied(true);
					}
				}
			} catch (error) {
				logError("Failed to paste from selection", error);
			}
		},
		[invalidatePasteRequest],
	);

	const deleteItem = useCallback(
		async (hash: string) => {
			invalidatePasteRequest();

			try {
				await invoke("delete_item", { hash });
				setSelectedItem((prev) => (prev && prev > 0 ? prev - 1 : null));
			} catch (error) {
				logError("Failed to delete clipboard item", error);
			}
		},
		[invalidatePasteRequest],
	);

	const clipboardMenuItems = useMemo(
		() =>
			clipboard.map((item) => ({
				label: item.text || `Image ${item.hash.slice(0, 8)}`,
				key: item.hash,
				onClick: () => pasteFromSelection(item.hash),
				preview: item.preview,
			})),
		[clipboard, pasteFromSelection],
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
						selectedItem !== null && selectedItem > 0
							? selectedItem - 1
							: clipboard.length - 1;

					break;
				case shortcuts.moveSelectionDown:
					event.preventDefault();

					newSelectedItem =
						selectedItem !== null && selectedItem < clipboard.length - 1
							? selectedItem + 1
							: 0;

					break;
				case shortcuts.pasteSelectedItem: {
					event.preventDefault();

					if (selectedItem !== null && isValidItem(selectedItem)) {
						pasteFromSelection(clipboard[selectedItem].hash);
					}

					return;
				}
				case shortcuts.deleteSelectedItem:
					event.preventDefault();

					if (selectedItem !== null && isValidItem(selectedItem)) {
						deleteItem(clipboard[selectedItem].hash);
					}

					break;
				default:
					break;
			}

			if (newSelectedItem !== null && newSelectedItem !== selectedItem) {
				historyRef.current?.children[newSelectedItem]?.scrollIntoView({
					block: "nearest",
				});
			}

			setSelectedItem(newSelectedItem);
		},
		[
			clipboard,
			selectedItem,
			pasteFromSelection,
			hide,
			deleteItem,
			shortcuts,
			invalidatePasteRequest,
		],
	);

	const handleBlur = useCallback(() => {
		setSelectedItem(null);
		setManualPasteCopied(false);
	}, []);

	const handleFocus = useCallback(() => {
		fetchClipboardHistory();
		invoke<ShortcutSettings>("get_shortcuts")
			.then(setShortcuts)
			.catch((error) => logError("Failed to get keyboard shortcuts", error));
	}, [fetchClipboardHistory]);

	useEffect(() => {
		loadCapabilities();
	}, [loadCapabilities]);

	useEffect(() => {
		const unlisten = listen<string>("clipboard-changed", async () => {
			const isVisible = await getCurrentWindow().isVisible();

			if (!isVisible) return;

			fetchClipboardHistory();
		});

		return () => {
			unlisten.then((unlisten) => unlisten());
		};
	}, [fetchClipboardHistory]);

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

				<MenuSeparator />

				{unavailableCapabilities.length > 0 && (
					<div className="menu__capabilities" role="status">
						{unavailableCapabilities.map((message) => (
							<p key={message}>{message}</p>
						))}
					</div>
				)}

				<div ref={historyRef} className="menu__history">
					{clipboardMenuItems.map((item, idx) => (
						<ListItem {...item} key={item.key} active={idx === selectedItem} />
					))}
				</div>
			</div>
		</div>
	);
}

function SettingsView() {
	const [saved, setSaved] = useState<ShortcutSettings | null>(null);
	const [draft, setDraft] = useState<ShortcutSettings | null>(null);
	const [recording, setRecording] = useState<ShortcutField | null>(null);
	const [error, setError] = useState<string | null>(null);
	const platformPresentation = getPlatformPresentation();

	const load = useCallback(
		() =>
			invoke<ShortcutSettings>("get_shortcuts")
				.then((settings) => {
					setSaved(settings);
					setDraft(settings);
				})
				.catch((reason) => {
					logError("Failed to load keyboard shortcuts", reason);
					setError(String(reason));
				}),
		[],
	);

	useEffect(() => {
		load();
		window.addEventListener("focus", load);
		return () => window.removeEventListener("focus", load);
	}, [load]);

	useEffect(() => {
		if (!recording) return;
		const record = (event: KeyboardEvent) => {
			event.preventDefault();
			event.stopPropagation();
			if (event.key === "Escape") {
				setRecording(null);
				getCurrentWindow()
					.hide()
					.catch((error) => logError("Failed to hide settings window", error));
				return;
			}
			const shortcut = shortcutFromEvent(event);
			if (!shortcut) {
				setError("Escape and modifier-only shortcuts cannot be used.");
				return;
			}
			setDraft((current) => current && { ...current, [recording]: shortcut });
			setError(null);
			setRecording(null);
		};
		window.addEventListener("keydown", record, true);
		return () => window.removeEventListener("keydown", record, true);
	}, [recording]);

	if (!draft || !saved) {
		if (error)
			return (
				<main
					className={`settings settings__error ${platformPresentation.className}`}
					role="alert"
				>
					{error}
				</main>
			);
		return (
			<main className={`settings ${platformPresentation.className}`}>
				Loading settings…
			</main>
		);
	}

	const save = async () => {
		try {
			const updated = await invoke<ShortcutSettings>("save_shortcuts", {
				settings: draft,
			});
			setSaved(updated);
			setDraft(updated);
			setError(null);
		} catch (reason) {
			logError("Failed to save keyboard shortcuts", reason);
			setError(String(reason));
		}
	};

	return (
		<main className={`settings ${platformPresentation.className}`}>
			<h1>Keyboard shortcuts</h1>
			<p>
				Click a shortcut, then press one key combination. Escape always closes
				Klipo.
			</p>
			{shortcutFields.map(([field, label]) => (
				<label className="settings__field" key={field}>
					<span>{label}</span>
					<button
						type="button"
						className={
							recording === field
								? "settings__shortcut is-recording"
								: "settings__shortcut"
						}
						onClick={() => {
							setRecording(field);
							setError(null);
						}}
					>
						{recording === field
							? "Press shortcut…"
							: shortcutLabel(draft[field])}
					</button>
				</label>
			))}
			{error && (
				<p className="settings__error" role="alert">
					{error}
				</p>
			)}
			<div className="settings__actions">
				<button
					type="button"
					onClick={save}
					disabled={JSON.stringify(saved) === JSON.stringify(draft)}
				>
					Save changes
				</button>
			</div>
		</main>
	);
}

export default function Root() {
	return getCurrentWindow().label === "settings" ? <SettingsView /> : <App />;
}
