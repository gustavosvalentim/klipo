import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	currentWindow: {
		isVisible: vi.fn(),
		label: "main",
		setFocus: vi.fn(),
		show: vi.fn(),
	},
	invoke: vi.fn(),
	listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({
	getCurrentWindow: () => mocks.currentWindow,
}));

import { App } from "./App";

function configurePaste(paste: () => Promise<unknown>) {
	mocks.invoke.mockImplementation((command: string) => {
		switch (command) {
			case "fetch_clipboard":
				return Promise.resolve([
					{ hash: "text:known", text: "Clipboard entry" },
				]);
			case "get_shortcuts":
				return Promise.resolve({
					version: 1,
					openKlipo: "SUPER+KeyV",
					moveSelectionUp: "ArrowUp",
					moveSelectionDown: "ArrowDown",
					pasteSelectedItem: "Enter",
					deleteSelectedItem: "Delete",
				});
			case "paste":
				return paste();
			default:
				return Promise.resolve();
		}
	});
}

async function renderPicker() {
	render(<App />);
	window.dispatchEvent(new Event("focus"));

	return screen.findByRole("button", { name: "Clipboard entry" });
}

function nextTick() {
	return new Promise((resolve) => setTimeout(resolve, 0));
}

function activateWithEnter(button: HTMLElement) {
	const defaultWasAllowed = fireEvent.keyDown(button, {
		code: "Enter",
		key: "Enter",
	});

	if (defaultWasAllowed) fireEvent.click(button);
}

describe("App", () => {
	beforeEach(() => {
		HTMLElement.prototype.scrollIntoView = vi.fn();
		mocks.currentWindow.isVisible.mockResolvedValue(true);
		mocks.currentWindow.show.mockResolvedValue(undefined);
		mocks.currentWindow.setFocus.mockResolvedValue(undefined);
		mocks.listen.mockResolvedValue(vi.fn());
		configurePaste(() => Promise.resolve("CopiedForManualPaste"));
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("keeps the picker available and announces copied when automatic paste is unavailable", async () => {
		const item = await renderPicker();
		fireEvent.click(item);

		await waitFor(() => {
			expect(screen.getByRole("status").textContent).toBe("Copied");
			expect(mocks.currentWindow.show).toHaveBeenCalledOnce();
			expect(mocks.currentWindow.setFocus).toHaveBeenCalledOnce();
		});

		fireEvent.blur(window);

		await waitFor(() => {
			expect(screen.queryByRole("status")).toBeNull();
		});

		fireEvent.keyDown(window, { key: "Escape" });

		await waitFor(() => {
			expect(screen.queryByRole("status")).toBeNull();
			expect(mocks.invoke).toHaveBeenCalledWith("close");
		});
	});

	it.each([
		"Pasted",
		"ClipboardWriteFailed",
	])("does not change picker presentation for %s", async (outcome) => {
		configurePaste(() => Promise.resolve(outcome));
		const item = await renderPicker();
		fireEvent.click(item);

		await nextTick();

		expect(screen.queryByRole("status")).toBeNull();
		expect(mocks.currentWindow.show).not.toHaveBeenCalled();
		expect(mocks.currentWindow.setFocus).not.toHaveBeenCalled();
	});

	it("does not announce manual paste when showing the picker fails", async () => {
		mocks.currentWindow.show.mockRejectedValueOnce(new Error("show failed"));
		const item = await renderPicker();
		fireEvent.click(item);

		await waitFor(() => {
			expect(mocks.currentWindow.show).toHaveBeenCalledOnce();
			expect(mocks.currentWindow.setFocus).toHaveBeenCalledOnce();
		});

		expect(screen.queryByRole("status")).toBeNull();
	});

	it("does not announce manual paste when focusing the picker fails", async () => {
		mocks.currentWindow.setFocus.mockRejectedValueOnce(
			new Error("focus failed"),
		);
		const item = await renderPicker();
		fireEvent.click(item);

		await waitFor(() => {
			expect(mocks.currentWindow.show).toHaveBeenCalledOnce();
			expect(mocks.currentWindow.setFocus).toHaveBeenCalledOnce();
		});

		expect(screen.queryByRole("status")).toBeNull();
	});

	it("does not recover a stale manual-paste outcome after Escape", async () => {
		let resolvePaste!: (outcome: unknown) => void;
		const paste = new Promise<unknown>((resolve) => {
			resolvePaste = resolve;
		});
		configurePaste(() => paste);
		const item = await renderPicker();
		fireEvent.click(item);

		fireEvent.keyDown(window, { key: "Escape" });
		resolvePaste("CopiedForManualPaste");
		await nextTick();

		expect(screen.queryByRole("status")).toBeNull();
		expect(mocks.currentWindow.show).not.toHaveBeenCalled();
		expect(mocks.currentWindow.setFocus).not.toHaveBeenCalled();
		expect(mocks.invoke).toHaveBeenCalledWith("close");
	});

	it("does not recover an older manual-paste outcome after a new paste action", async () => {
		let resolveFirstPaste!: (outcome: unknown) => void;
		const firstPaste = new Promise<unknown>((resolve) => {
			resolveFirstPaste = resolve;
		});
		let pasteCalls = 0;
		configurePaste(() => {
			pasteCalls += 1;
			return pasteCalls === 1 ? firstPaste : Promise.resolve("Pasted");
		});
		const item = await renderPicker();
		fireEvent.click(item);
		fireEvent.click(item);

		resolveFirstPaste("CopiedForManualPaste");
		await nextTick();

		expect(screen.queryByRole("status")).toBeNull();
		expect(mocks.currentWindow.show).not.toHaveBeenCalled();
		expect(mocks.currentWindow.setFocus).not.toHaveBeenCalled();
	});

	it("activates settings and quit controls with Enter without a tray", async () => {
		await renderPicker();

		const settings = screen.getByRole("button", { name: "Settings" });
		const quit = screen.getByRole("button", { name: "Quit" });

		settings.focus();
		expect(document.activeElement).toBe(settings);
		activateWithEnter(settings);
		activateWithEnter(quit);

		expect(mocks.invoke).toHaveBeenCalledWith("show_settings");
		expect(mocks.invoke).toHaveBeenCalledWith("quit");
	});

	it("closes the picker when Escape is pressed from a focused control", async () => {
		await renderPicker();

		const settings = screen.getByRole("button", { name: "Settings" });
		settings.focus();
		fireEvent.keyDown(settings, { code: "Escape", key: "Escape" });

		await waitFor(() => {
			expect(mocks.invoke).toHaveBeenCalledWith("close");
		});
	});

	it("keeps picker shortcuts active outside interactive controls", async () => {
		await renderPicker();

		fireEvent.keyDown(window, { code: "ArrowDown", key: "ArrowDown" });
		fireEvent.keyDown(window, { code: "Enter", key: "Enter" });

		await waitFor(() => {
			expect(mocks.invoke).toHaveBeenCalledWith("paste", {
				hash: "text:known",
			});
		});
	});
});
