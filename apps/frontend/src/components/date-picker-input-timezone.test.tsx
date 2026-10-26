import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DatePickerInput } from "@wealthfolio/ui/components/ui/date-picker-input";

describe("DatePickerInput timezone", () => {
  it("edits in the application timezone even across a browser DST gap", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn<(value: Date | undefined) => void>();
    render(
      <DatePickerInput
        aria-label="Completion"
        enableTime
        timeZone="Asia/Shanghai"
        value={new Date("2026-03-07T18:30:00Z")}
        onChange={onChange}
      />,
    );
    const hour = await screen.findByRole("spinbutton", { name: /hour/i });
    expect(hour).toHaveAttribute("aria-valuenow", "2");
    await user.click(screen.getByRole("spinbutton", { name: /minute/i }));
    await user.keyboard("{ArrowUp}");
    await waitFor(() => expect(onChange).toHaveBeenCalled());
    expect(onChange.mock.lastCall![0]?.toISOString()).toBe("2026-03-07T18:31:00.000Z");
  });
  it("keeps browser-local behavior when no timezone is supplied", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn<(value: Date | undefined) => void>();
    render(
      <DatePickerInput
        aria-label="Completion"
        enableTime
        value={new Date(2026, 9, 26, 10, 30)}
        onChange={onChange}
      />,
    );
    const hour = await screen.findByRole("spinbutton", { name: /hour/i });
    expect(hour).toHaveAttribute("aria-valuenow", "10");
    await user.click(screen.getByRole("spinbutton", { name: /minute/i }));
    await user.keyboard("{ArrowUp}");
    await waitFor(() => expect(onChange).toHaveBeenCalled());
    expect(onChange).toHaveBeenLastCalledWith(new Date(2026, 9, 26, 10, 31));
  });
});
