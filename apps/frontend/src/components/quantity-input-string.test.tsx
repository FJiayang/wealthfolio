import { fireEvent, render, screen } from "@testing-library/react";
import { FormattingProvider, QuantityInput } from "@wealthfolio/ui";
import { describe, expect, it, vi } from "vitest";

describe("QuantityInput exact strings", () => {
  it("preserves decimal strings beyond Number precision", () => {
    const onValueStringChange = vi.fn();
    render(
      <FormattingProvider locale="en-US">
        <QuantityInput
          aria-label="quantity"
          value="9007199254740993.12345678"
          onValueStringChange={onValueStringChange}
        />
      </FormattingProvider>,
    );
    expect(screen.getByRole("textbox", { name: "quantity" })).toHaveValue(
      "9,007,199,254,740,993.12345678",
    );
  });
  it("emits machine decimal strings for localized edits", () => {
    const onValueStringChange = vi.fn();
    render(
      <FormattingProvider locale="de-DE">
        <QuantityInput aria-label="quantity" value="" onValueStringChange={onValueStringChange} />
      </FormattingProvider>,
    );
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "1234,5678" } });
    expect(onValueStringChange).toHaveBeenLastCalledWith("1234.5678");
  });
});
