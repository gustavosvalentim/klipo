import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

import { SettingsView } from "./SettingsView";

const settings = {
	version: 1,
	openKlipo: "SUPER+SHIFT+KeyV",
	moveSelectionUp: "ArrowUp",
	moveSelectionDown: "ArrowDown",
	pasteSelectedItem: "Enter",
	deleteSelectedItem: "Delete",
};

describe("SettingsView", () => {
	beforeEach(() => {
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "get_shortcuts") return Promise.resolve(settings);
			if (command === "get_capabilities")
				return Promise.resolve({ session: "x11" });
			return Promise.resolve();
		});
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("preserves an unsaved shortcut when the window regains focus", async () => {
		render(<SettingsView />);
		const shortcut = (await screen.findByText("ArrowUp")) as HTMLButtonElement;
		fireEvent.click(shortcut);
		fireEvent.keyDown(window, { key: "w", code: "KeyW" });
		await screen.findByText("W");
		fireEvent.focus(window);
		expect(screen.getByText("W")).toBeTruthy();
		expect(
			mocks.invoke.mock.calls.filter(([name]) => name === "get_shortcuts"),
		).toHaveLength(1);
	});

	it("does not replace edits with an older focus refresh", async () => {
		let resolveFocusLoad!: (loadedSettings: typeof settings) => void;
		const focusLoad = new Promise<typeof settings>((resolve) => {
			resolveFocusLoad = resolve;
		});
		let loads = 0;
		mocks.invoke.mockImplementation((command: string) => {
			if (command === "get_shortcuts") {
				loads += 1;
				return loads === 1 ? Promise.resolve(settings) : focusLoad;
			}
			if (command === "get_capabilities")
				return Promise.resolve({ session: "x11" });
			return Promise.resolve();
		});
		render(<SettingsView />);
		const shortcut = (await screen.findByText("ArrowUp")) as HTMLButtonElement;
		fireEvent.focus(window);
		fireEvent.click(shortcut);
		fireEvent.keyDown(window, { key: "w", code: "KeyW" });
		await screen.findByText("W");
		resolveFocusLoad(settings);
		await waitFor(() => expect(screen.getByText("W")).toBeTruthy());
	});

	it("closes on Escape when no shortcut is being recorded", async () => {
		render(<SettingsView />);
		await screen.findByText("Keyboard shortcuts");
		fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
		await waitFor(() =>
			expect(mocks.invoke).toHaveBeenCalledWith("close_settings"),
		);
	});

	it("leaves keyboard navigation and button activation available outside recording", async () => {
		render(<SettingsView />);
		const shortcut = await screen.findByText("ArrowUp");

		expect(fireEvent.keyDown(shortcut, { key: "Tab", code: "Tab" })).toBe(true);
		expect(fireEvent.keyDown(shortcut, { key: "Enter", code: "Enter" })).toBe(
			true,
		);

		fireEvent.click(shortcut);
		expect(fireEvent.keyDown(window, { key: "w", code: "KeyW" })).toBe(false);
		await screen.findByText("W");
		const save = screen.getByRole("button", { name: "Save changes" });
		expect(fireEvent.keyDown(save, { key: " ", code: "Space" })).toBe(true);
	});
});
