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
			expect(mocks.currentWindow.show).not.toHaveBeenCalled();
			expect(mocks.currentWindow.setFocus).not.toHaveBeenCalled();
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
	])("does not change picker presentation for %s", async (outcome) => {
		configurePaste(() => Promise.resolve(outcome));
		const item = await renderPicker();
		fireEvent.click(item);

		await nextTick();

		expect(screen.queryByRole("status")).toBeNull();
		expect(mocks.currentWindow.show).not.toHaveBeenCalled();
		expect(mocks.currentWindow.setFocus).not.toHaveBeenCalled();
	});

	it("shows a clipboard write failure", async () => {
		configurePaste(() =>
			Promise.reject(new Error("Could not copy this item.")),
		);
		const item = await renderPicker();
		fireEvent.click(item);
		await waitFor(() =>
			expect(screen.getByRole("alert").textContent).toBe(
				"Could not copy this item.",
			),
		);
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

	it("shows hover or keyboard selection, but not both", async () => {
		const item = await renderPicker();
		const history = item.closest(".menu__history");
		const row = item.parentElement as HTMLElement;
		row.scrollIntoView = vi.fn();

		fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown" });

		expect(item.className).toContain("is-active");
		expect(history?.className).toContain("has-keyboard-selection");

		fireEvent.pointerMove(history as Element);

		expect(item.className).not.toContain("is-active");
		expect(history?.className).not.toContain("has-keyboard-selection");
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

	it("loads history on mount and preserves it when a later fetch fails", async () => {
		let fetches = 0;
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "fetch_clipboard") {
				fetches += 1;
				return fetches === 1
					? Promise.resolve([{ hash: "text:known", text: "Clipboard entry" }])
					: Promise.reject(new Error("database unavailable"));
			}
			return Promise.resolve();
		});
		render(<App />);
		await screen.findByRole("button", { name: "Clipboard entry" });
		fireEvent.focus(window);
		await waitFor(() =>
			expect(screen.getByRole("alert").textContent).toContain("Could not load"),
		);
		expect(
			screen.getByRole("button", { name: "Clipboard entry" }),
		).toBeTruthy();
	});

	it("keeps the selected identity when history is inserted or reordered", async () => {
		const first = [
			{ hash: "a", text: "A" },
			{ hash: "b", text: "B" },
		];
		const second = [
			{ hash: "c", text: "C" },
			{ hash: "b", text: "B" },
			{ hash: "a", text: "A" },
		];
		let history = first;
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "fetch_clipboard") return Promise.resolve(history);
			if (command === "get_shortcuts")
				return Promise.resolve({
					moveSelectionDown: "ArrowDown",
					pasteSelectedItem: "Enter",
				});
			if (command === "paste") return Promise.resolve("Pasted");
			return Promise.resolve();
		});
		render(<App />);
		await screen.findByRole("button", { name: "B" });
		fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown" });
		fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown" });
		history = second;
		fireEvent.focus(window);
		await screen.findByRole("button", { name: "C" });
		fireEvent.keyDown(window, { key: "Enter", code: "Enter" });
		await waitFor(() =>
			expect(mocks.invoke).toHaveBeenCalledWith("paste", { hash: "b" }),
		);
	});

	it("selects the preceding survivor after deleting the selected row", async () => {
		let history = [
			{ hash: "a", text: "A" },
			{ hash: "b", text: "B" },
			{ hash: "c", text: "C" },
		];
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "fetch_clipboard") return Promise.resolve(history);
			if (command === "get_shortcuts")
				return Promise.resolve({
					moveSelectionDown: "ArrowDown",
					deleteSelectedItem: "Delete",
					pasteSelectedItem: "Enter",
				});
			if (command === "delete_item") {
				history = [history[0], history[2]];
				return Promise.resolve("Deleted");
			}
			if (command === "paste") return Promise.resolve("Pasted");
			return Promise.resolve();
		});
		render(<App />);
		await screen.findByRole("button", { name: "B" });
		fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown" });
		fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown" });
		fireEvent.keyDown(window, { key: "Delete", code: "Delete" });
		await waitFor(() =>
			expect(screen.queryByRole("button", { name: "B" })).toBeNull(),
		);
		fireEvent.keyDown(window, { key: "Enter", code: "Enter" });
		await waitFor(() =>
			expect(mocks.invoke).toHaveBeenCalledWith("paste", { hash: "a" }),
		);
	});

	it("keeps selection after a failed delete", async () => {
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "fetch_clipboard")
				return Promise.resolve([{ hash: "a", text: "A" }]);
			if (command === "get_shortcuts")
				return Promise.resolve({
					moveSelectionDown: "ArrowDown",
					deleteSelectedItem: "Delete",
					pasteSelectedItem: "Enter",
				});
			if (command === "delete_item")
				return Promise.reject(new Error("database unavailable"));
			if (command === "paste") return Promise.resolve("Pasted");
			return Promise.resolve();
		});
		render(<App />);
		await screen.findByRole("button", { name: "A" });
		fireEvent.keyDown(window, { key: "ArrowDown", code: "ArrowDown" });
		fireEvent.keyDown(window, { key: "Delete", code: "Delete" });
		await screen.findByRole("alert");
		fireEvent.keyDown(window, { key: "Enter", code: "Enter" });
		await waitFor(() =>
			expect(mocks.invoke).toHaveBeenCalledWith("paste", { hash: "a" }),
		);
	});

	it("ignores an older fetch that resolves after a newer one", async () => {
		let resolveOld!: (items: unknown[]) => void;
		const oldFetch = new Promise<unknown[]>((resolve) => {
			resolveOld = resolve;
		});
		let fetches = 0;
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "fetch_clipboard") {
				fetches += 1;
				return fetches === 1
					? oldFetch
					: Promise.resolve([{ hash: "new", text: "New" }]);
			}
			return Promise.resolve();
		});
		render(<App />);
		fireEvent.focus(window);
		await screen.findByRole("button", { name: "New" });
		resolveOld([{ hash: "old", text: "Old" }]);
		await nextTick();
		expect(screen.queryByRole("button", { name: "Old" })).toBeNull();
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
