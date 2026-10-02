export type ShortcutSettings = {
	version: number;
	openKlipo: string;
	moveSelectionUp: string;
	moveSelectionDown: string;
	pasteSelectedItem: string;
	deleteSelectedItem: string;
};

export type ShortcutField = Exclude<keyof ShortcutSettings, "version">;

export const shortcutFields: Array<[ShortcutField, string]> = [
	["openKlipo", "Open Klipo"],
	["moveSelectionUp", "Move selection up"],
	["moveSelectionDown", "Move selection down"],
	["pasteSelectedItem", "Paste selected item"],
	["deleteSelectedItem", "Delete selected item"],
];

const modifierKeys = new Set(["Meta", "Control", "Alt", "Shift"]);

export function shortcutFromEvent(event: KeyboardEvent) {
	if (modifierKeys.has(event.key) || event.key === "Escape") return null;
	const modifiers = [
		event.metaKey && "SUPER",
		event.ctrlKey && "CTRL",
		event.altKey && "ALT",
		event.shiftKey && "SHIFT",
	].filter(Boolean);
	return [...modifiers, event.code].join("+");
}
