import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ClearHistoryButton } from "./ClearHistoryButton";

describe("ClearHistoryButton", () => {
	afterEach(cleanup);

	it("exposes its purpose without changing click behavior", () => {
		let clicked = false;

		render(<ClearHistoryButton onClick={() => (clicked = true)} />);

		const button = screen.getByRole("button", { name: "Clear history" });
		const icon = button.querySelector("svg");
		const tooltip = screen.getByRole("tooltip", { name: "Clear history" });
		const tooltipContainer = tooltip.parentElement;

		button.focus();

		expect(document.activeElement).toBe(button);
		expect(tooltipContainer?.contains(document.activeElement)).toBe(true);
		expect(tooltip.className).toContain("group-focus-within:opacity-100");

		fireEvent.click(button);

		expect(clicked).toBe(true);
		expect(icon?.getAttribute("aria-hidden")).toBe("true");
		expect(icon?.getAttribute("focusable")).toBe("false");
		expect(tooltip.className).toContain("group-hover:opacity-100");
	});
});
