type PlatformName = "linux" | "macos";

type PlatformPresentation = {
	className: `platform-${PlatformName}`;
	shortcutLabels: Record<string, string>;
	shortcutSeparator: string;
};

const macosShortcutLabels = {
	SUPER: "⌘",
	CTRL: "⌃",
	ALT: "⌥",
	SHIFT: "⇧",
	ArrowUp: "↑",
	ArrowDown: "↓",
	ArrowLeft: "←",
	ArrowRight: "→",
	Enter: "↩",
	Backspace: "⌫",
	Delete: "⌦",
};

const linuxShortcutLabels = {
	SUPER: "Super",
	CTRL: "Ctrl",
	ALT: "Alt",
	SHIFT: "Shift",
};

const platformPresentations: Record<PlatformName, PlatformPresentation> = {
	macos: {
		className: "platform-macos",
		shortcutLabels: macosShortcutLabels,
		shortcutSeparator: "",
	},
	linux: {
		className: "platform-linux",
		shortcutLabels: linuxShortcutLabels,
		shortcutSeparator: "+",
	},
};

export function getPlatformPresentation(
	platform = globalThis.navigator?.platform,
): PlatformPresentation {
	const platformName = platform?.toLowerCase().includes("mac")
		? "macos"
		: "linux";

	return platformPresentations[platformName];
}

function shortcutKeyLabel(key: string, shortcutLabels: Record<string, string>) {
	return (
		shortcutLabels[key] ??
		key.replace(/^Key([A-Z])$/, "$1").replace(/^Digit([0-9])$/, "$1")
	);
}

export function shortcutLabel(shortcut: string, platform?: string) {
	const presentation = getPlatformPresentation(platform);
	const labels = shortcut
		.split("+")
		.map((key) => shortcutKeyLabel(key, presentation.shortcutLabels));

	return labels.join(presentation.shortcutSeparator);
}
