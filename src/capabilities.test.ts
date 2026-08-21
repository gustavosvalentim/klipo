import { describe, expect, it } from "vitest";
import {
	type DesktopCapabilities,
	unavailableCapabilityMessages,
} from "./capabilities";

function allAvailable(): DesktopCapabilities {
	return {
		session: "x11",
		clipboardRead: { status: "available" },
		clipboardWrite: { status: "available" },
		watcher: { status: "available" },
		shortcut: { status: "available" },
		pointer: { status: "available" },
		targetRestoration: { status: "available" },
		input: { status: "available" },
		automaticPaste: { status: "available" },
		tray: { status: "available" },
	};
}

describe("desktop capability presentation", () => {
	it("reports an enabled session without warnings", () => {
		expect(unavailableCapabilityMessages(allAvailable())).toEqual([]);
	});

	it("distinguishes partial failures with their actionable reasons", () => {
		const capabilities = allAvailable();
		capabilities.shortcut = {
			status: "unavailable",
			reason: "unsupported_session",
		};
		capabilities.automaticPaste = {
			status: "unavailable",
			reason: "initialization_failed",
		};

		expect(unavailableCapabilityMessages(capabilities)).toEqual([
			"Global shortcut unavailable: This desktop session does not support this integration.",
			"Automatic paste unavailable: Klipo could not start this integration. Restart Klipo and check desktop permissions.",
		]);
	});

	it("describes every unavailable integration when the desktop is unusable", () => {
		const unavailable = {
			status: "unavailable" as const,
			reason: "unknown_session" as const,
		};
		const capabilities: DesktopCapabilities = {
			session: "unknown",
			clipboardRead: unavailable,
			clipboardWrite: unavailable,
			watcher: unavailable,
			shortcut: unavailable,
			pointer: unavailable,
			targetRestoration: unavailable,
			input: unavailable,
			automaticPaste: unavailable,
			tray: unavailable,
		};

		expect(unavailableCapabilityMessages(capabilities)).toHaveLength(9);
		expect(unavailableCapabilityMessages(capabilities)[0]).toContain(
			"Klipo could not identify the current desktop session",
		);
	});
});
