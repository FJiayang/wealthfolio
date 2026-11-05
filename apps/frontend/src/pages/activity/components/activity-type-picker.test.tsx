import { ActivityType } from "@/lib/constants";
import { render, screen } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { DirectionProvider } from "@wealthfolio/ui";
import { useEffect, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { ActivityTypePicker } from "./activity-type-picker";

const carousel = vi.hoisted(() => ({
  scrollTo: vi.fn(),
  canScrollPrev: () => false,
  canScrollNext: () => false,
  on: vi.fn(),
}));
vi.mock("@wealthfolio/ui/components/ui/carousel", () => ({
  Carousel: ({
    children,
    setApi,
  }: {
    children: ReactNode;
    setApi: (api: typeof carousel) => void;
  }) => {
    useEffect(() => {
      setApi(carousel);
    }, [setApi]);
    return <div>{children}</div>;
  },
  CarouselContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  CarouselItem: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  CarouselNext: () => null,
  CarouselPrevious: () => null,
}));

it("scrolls to the initial selection without moving types away on subsequent selections", () => {
  carousel.scrollTo.mockClear();
  const view = render(<ActivityTypePicker value={ActivityType.BUY} onSelect={vi.fn()} />);
  expect(carousel.scrollTo).toHaveBeenCalledWith(0, true);
  view.rerender(<ActivityTypePicker value={ActivityType.SELL} onSelect={vi.fn()} />);
  expect(carousel.scrollTo).toHaveBeenCalledTimes(1);
});

describe("ActivityTypePicker reclassification types", () => {
  it("keeps ADJUSTMENT out of the ordinary creation picker", () => {
    render(<ActivityTypePicker onSelect={vi.fn()} />);

    expect(screen.queryByTestId("activity-type-adjustment")).not.toBeInTheDocument();
  });

  it("offers every editable canonical target while reclassifying", () => {
    render(<ActivityTypePicker onSelect={vi.fn()} includeReclassificationTypes />);

    expect(screen.getByTestId(`activity-type-${ActivityType.CREDIT.toLowerCase()}`)).toBeVisible();
    expect(
      screen.getByTestId(`activity-type-${ActivityType.ADJUSTMENT.toLowerCase()}`),
    ).toBeVisible();
  });

  it("keeps a conversion, which is two rows, out of reclassifying one row", () => {
    render(<ActivityTypePicker onSelect={vi.fn()} includeReclassificationTypes />);

    expect(screen.queryByTestId("activity-type-conversion")).not.toBeInTheDocument();
  });
});

describe("ActivityTypePicker grid keyboard", () => {
  it.each([
    ["ltr", "{ArrowRight}", "{ArrowLeft}"],
    ["rtl", "{ArrowLeft}", "{ArrowRight}"],
  ] as const)(
    "moves to the next type with the arrow pointing forward (%s)",
    async (direction, next, previous) => {
      const user = userEvent.setup();
      render(
        <DirectionProvider direction={direction}>
          <ActivityTypePicker onSelect={vi.fn()} />
        </DirectionProvider>,
      );
      await user.click(screen.getByRole("button", { name: "Expand to show all types" }));
      const grid = screen.getByRole("group", { name: "All activity types" });
      const [first, second] = Array.from(grid.querySelectorAll("button"));

      first.focus();
      await user.keyboard(next);
      expect(second).toHaveFocus();
      await user.keyboard(previous);
      expect(first).toHaveFocus();
    },
  );
});
