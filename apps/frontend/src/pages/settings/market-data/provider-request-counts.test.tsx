import { fireEvent, render, screen } from "@/test/render";
import { describe, expect, it, vi } from "vitest";
import { FormattingProvider } from "@wealthfolio/ui";
import { ProviderTodayRequests } from "./provider-request-counts";

describe("provider request count entry", () => {
  it("keeps zero counts clickable and unavailable counts disabled", () => {
    const open = vi.fn();
    const { rerender } = render(<ProviderTodayRequests count={0} onClick={open} />);
    fireEvent.click(screen.getByRole("button", { name: "Today so far: 0" }));
    expect(open).toHaveBeenCalledOnce();
    rerender(<ProviderTodayRequests count={undefined} onClick={open} />);
    expect(screen.getByRole("button")).toBeDisabled();
  });
  it("formats counts and never substitutes zero for unavailable", () => {
    const { rerender } = render(
      <FormattingProvider locale="ar-EG">
        <ProviderTodayRequests count={1234} />
      </FormattingProvider>,
    );
    expect(screen.getByText(/١٬٢٣٤/)).toBeInTheDocument();
    rerender(<ProviderTodayRequests count={undefined} />);
    expect(screen.getByText(/—/)).toBeInTheDocument();
  });
});
