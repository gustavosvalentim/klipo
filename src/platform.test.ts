import { describe, expect, it } from "vitest";
import {
	getPlatformPresentation,
	presentationForSession,
	shortcutLabel,
} from "./platform";

describe("platform presentation", () => {
	it("uses readable Linux shortcut terminology and opaque presentation styles", () => {
		const presentation = getPlatformPresentation("Linux x86_64");

		expect(presentation.className).toBe("platform-linux");
		expect(
			shortcutLabel("SUPER+CTRL+ALT+SHIFT+Enter+Delete", "Linux x86_64"),
		).toBe("Super+Ctrl+Alt+Shift+Enter+Delete");
	});

	it("preserves macOS shortcut glyphs and presentation styles", () => {
		const presentation = getPlatformPresentation("MacIntel");

		expect(presentation.className).toBe("platform-macos");
		expect(shortcutLabel("SUPER+SHIFT+KeyV", "MacIntel")).toBe("⌘⇧V");
		expect(shortcutLabel("CTRL+ArrowDown+Backspace", "MacIntel")).toBe("⌃↓⌫");
	});

	it("uses the native session for platform presentation", () => {
		expect(presentationForSession("macos").className).toBe("platform-macos");
		expect(presentationForSession("wayland").className).toBe("platform-linux");
	});
});
