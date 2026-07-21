import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Trash2 } from "react-feather";
import { ListItem } from "./components/ListItem";
import "./App.css";

type ClipboardItem = {
	hash: string;
	text: string;
};

type Clipboard = ClipboardItem[];

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

function shortcutLabel(shortcut: string) {
	return shortcut
		.replace(/SUPER/g, "⌘")
		.replace(/CTRL/g, "⌃")
		.replace(/ALT/g, "⌥")
		.replace(/SHIFT/g, "⇧")
		.replace(/ArrowUp/g, "↑")
		.replace(/ArrowDown/g, "↓")
		.replace(/ArrowLeft/g, "←")
		.replace(/ArrowRight/g, "→")
		.replace(/Enter/g, "↩")
		.replace(/Backspace/g, "⌫")
		.replace(/Delete/g, "⌦")
		.replace(/Key([A-Z])/g, "$1")
		.replace(/Digit([0-9])/g, "$1")
		.replace(/\+/g, "");
}

const MenuSeparator = () => (
	<div className="h-px my-[4px] mx-0 bg-[rgba(235,235,245,0.18)]" />
);

function App() {
	const [clipboard, setClipboard] = useState<Clipboard>([]);
	const [selectedItem, setSelectedItem] = useState<number | null>(null);
	const [shortcuts, setShortcuts] = useState<ShortcutSettings | null>(null);

	const hide = useCallback(() => invoke("close"), []);

	const fetchClipboardHistory = useCallback(async () => {
		try {
			const clipboard = await invoke<ClipboardItem[]>("fetch_clipboard");
			setClipboard(clipboard);
		} catch (error) {
			console.error("Failed to get clipboard history", error);
		}
	}, []);

	const clearHistory = useCallback(async () => {
		try {
			await invoke("clear");
		} catch (error) {
			console.error("Failed to clear clipboard history", error);
		}
	}, []);

	const pasteFromSelection = useCallback(async (text: string) => {
		try {
			await invoke("paste", { text });
		} catch (error) {
			console.error("Failed to paste from selection", error);
		}
	}, []);

	const deleteItem = useCallback(async (text: string) => {
		await invoke("delete_item", { text });
		setSelectedItem((prev) => (prev && prev > 0 ? prev - 1 : null));
	}, []);

	const actionsMenuItems = [
		{ label: "Clear History", onClick: clearHistory, icon: Trash2 },
	];

	const clipboardMenuItems = useMemo(
		() =>
			clipboard.map((item, idx) => ({
				label: `${idx}. ${item.text}`,
				key: item.hash,
				onClick: () => pasteFromSelection(item.text),
			})),
		[clipboard, pasteFromSelection],
	);

	const handleKeyDown = useCallback(
		(event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				hide();
				return;
			}
			if (!shortcuts) return;
			const isValidItem = (itemIdx: number) =>
				itemIdx >= 0 && itemIdx < clipboard.length;

			let newSelectedItem = selectedItem;

			const pressedShortcut = shortcutFromEvent(event);
			switch (pressedShortcut) {
				case shortcuts.moveSelectionUp:
					event.preventDefault();
					newSelectedItem =
						selectedItem !== null ? selectedItem - 1 : clipboard.length;
					break;
				case shortcuts.moveSelectionDown:
					event.preventDefault();
					newSelectedItem = selectedItem !== null ? selectedItem + 1 : 0;
					break;
				case shortcuts.pasteSelectedItem: {
					event.preventDefault();

					if (selectedItem !== null && isValidItem(selectedItem)) {
						pasteFromSelection(clipboard[selectedItem].text);
					}

					return;
				}
				case shortcuts.deleteSelectedItem:
					event.preventDefault();

					if (selectedItem !== null && isValidItem(selectedItem)) {
						deleteItem(clipboard[selectedItem].text);
					}

					break;
				default:
					break;
			}

			if (newSelectedItem === null || !isValidItem(newSelectedItem)) {
				newSelectedItem = 0;
			}

			setSelectedItem(newSelectedItem);
		},
		[clipboard, selectedItem, pasteFromSelection, hide, deleteItem, shortcuts],
	);

	const handleBlur = useCallback(() => {
		setSelectedItem(null);
	}, []);

	const handleFocus = useCallback(() => {
		fetchClipboardHistory();
		invoke<ShortcutSettings>("get_shortcuts")
			.then(setShortcuts)
			.catch(console.error);
	}, [fetchClipboardHistory]);

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
		<div className="menu text-gray-100/80">
			<div className="menu__content">
				<div className="flex justify-between items-center mx-2">
					<div className="flex justify-left items-center">
						<span className="text-base font-bold">Klipo</span>
					</div>

					<div className="flex justify-right items-center">
						{actionsMenuItems.map((item) => (
							<button
								type="button"
								className="cursor-pointer p-1 rounded-md hover:bg-[#0a84ff]"
								onClick={item.onClick}
								key={item.label}
							>
								<item.icon className="w-4 h-4" />
							</button>
						))}
					</div>
				</div>

				<MenuSeparator />

				<div className="menu__history">
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

	const load = useCallback(
		() =>
			invoke<ShortcutSettings>("get_shortcuts")
				.then((settings) => {
					setSaved(settings);
					setDraft(settings);
				})
				.catch((reason) => setError(String(reason))),
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
				getCurrentWindow().hide().catch(console.error);
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
				<main className="settings settings__error" role="alert">
					{error}
				</main>
			);
		return <main className="settings">Loading settings…</main>;
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
			setError(String(reason));
		}
	};

	return (
		<main className="settings">
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
