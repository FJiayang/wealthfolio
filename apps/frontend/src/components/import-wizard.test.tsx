import { fireEvent, render, screen } from "@testing-library/react";
import {
  FileDropzone,
  FormattingProvider,
  ImportWizardLayout,
  StepNavigation,
} from "@wealthfolio/ui";
import { describe, expect, it, vi } from "vitest";

const labels = {
  choose: "Choose",
  remove: "Remove",
  processing: "Processing",
  drop: "Drop",
  hint: "PDF",
};
describe("shared import wizard", () => {
  it("accepts drag and drop, and ignores drops while loading or a file is selected", () => {
    const onFileChange = vi.fn();
    const file = new File(["test"], "test.pdf");
    const { rerender } = render(
      <FormattingProvider locale="en-US">
        <FileDropzone file={null} onFileChange={onFileChange} labels={labels} />
      </FormattingProvider>,
    );
    fireEvent.drop(screen.getByRole("button", { name: "Choose" }), {
      dataTransfer: { files: [file] },
    });
    expect(onFileChange).toHaveBeenCalledWith(file);
    onFileChange.mockClear();
    rerender(
      <FormattingProvider locale="en-US">
        <FileDropzone file={null} isLoading onFileChange={onFileChange} labels={labels} />
      </FormattingProvider>,
    );
    fireEvent.drop(screen.getByRole("button", { name: "Choose" }), {
      dataTransfer: { files: [file] },
    });
    expect(onFileChange).not.toHaveBeenCalled();
  });
  it("removes a selected file and hides steps on results", () => {
    const onFileChange = vi.fn();
    const { rerender } = render(
      <FormattingProvider locale="en-US">
        <FileDropzone
          file={new File(["test"], "test.pdf")}
          onFileChange={onFileChange}
          labels={labels}
        />
      </FormattingProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onFileChange).toHaveBeenCalledWith(null);
    rerender(
      <ImportWizardLayout isResult steps={<span>Steps</span>} navigation={<span>Navigation</span>}>
        <span>Result</span>
      </ImportWizardLayout>,
    );
    expect(screen.queryByText("Steps")).not.toBeInTheDocument();
    expect(screen.getByText("Navigation")).toBeInTheDocument();
  });
  it("disables submission and displays the loading label", () => {
    render(
      <StepNavigation
        onNext={vi.fn()}
        onBack={vi.fn()}
        canGoBack
        canGoNext
        isNextLoading
        nextLabel="Next"
        nextLoadingLabel="Checking"
        backLabel="Back"
      />,
    );
    expect(screen.getByRole("button", { name: "Checking" })).toBeDisabled();
  });
});
