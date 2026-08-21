export type CapabilityUnavailableReason =
	| "unsupported_session"
	| "unknown_session"
	| "adapter_unavailable"
	| "initialization_failed";

export type CapabilityStatus =
	| { status: "available" }
	| { status: "unavailable"; reason: CapabilityUnavailableReason };

export type DesktopCapabilities = {
	session: "x11" | "wayland" | "unknown";
	clipboardRead: CapabilityStatus;
	clipboardWrite: CapabilityStatus;
	watcher: CapabilityStatus;
	shortcut: CapabilityStatus;
	pointer: CapabilityStatus;
	targetRestoration: CapabilityStatus;
	input: CapabilityStatus;
	automaticPaste: CapabilityStatus;
	tray: CapabilityStatus;
};

type CapabilityName = Exclude<keyof DesktopCapabilities, "session">;

const capabilityLabels: Record<CapabilityName, string> = {
	clipboardRead: "Clipboard reading",
	clipboardWrite: "Clipboard writing",
	watcher: "Clipboard watcher",
	shortcut: "Global shortcut",
	pointer: "Pointer positioning",
	targetRestoration: "Target restoration",
	input: "Paste input",
	automaticPaste: "Automatic paste",
	tray: "Tray icon",
};

const unavailableReasonMessages: Record<CapabilityUnavailableReason, string> = {
	unsupported_session:
		"This desktop session does not support this integration.",
	unknown_session:
		"Klipo could not identify the current desktop session. Set a supported X11 or Wayland session and restart Klipo.",
	adapter_unavailable:
		"The required desktop integration is unavailable in this session.",
	initialization_failed:
		"Klipo could not start this integration. Restart Klipo and check desktop permissions.",
};

export function unavailableCapabilityMessages(
	capabilities: DesktopCapabilities,
) {
	return (Object.keys(capabilityLabels) as CapabilityName[]).flatMap((name) => {
		const status = capabilities[name];

		return status.status === "unavailable"
			? [
					`${capabilityLabels[name]} unavailable: ${unavailableReasonMessages[status.reason]}`,
				]
			: [];
	});
}
