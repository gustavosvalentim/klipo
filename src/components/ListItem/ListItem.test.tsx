import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ListItem } from "./ListItem";

describe("ListItem", () => {
	afterEach(cleanup);

	it("uses stable classes for selected row presentation", () => {
		render(<ListItem active label="Selected clipboard item" />);

		const item = screen.getByRole("button", {
			name: "Selected clipboard item",
		});

		expect(item.className).toContain("list-item__button");
		expect(item.className).toContain("is-active");
	});
});
