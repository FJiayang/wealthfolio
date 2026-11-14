import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FileDropzone } from "./file-dropzone";

describe("native import file dropzone", () => {
  it("accepts replacement files while idle or loading", () => {
    const onFileChange = vi.fn();
    const file = new File(["synthetic"], "test.csv");
    const { container, rerender } = render(
      <FileDropzone file={null} onFileChange={onFileChange} />,
    );
    const drop = (value = file) =>
      fireEvent.drop(container.firstChild!, { dataTransfer: { files: [value] } });
    drop();
    expect(onFileChange).toHaveBeenCalledWith(file);
    onFileChange.mockClear();
    rerender(<FileDropzone file={null} isLoading onFileChange={onFileChange} />);
    drop();
    expect(onFileChange).toHaveBeenCalledExactlyOnceWith(file);
    onFileChange.mockClear();
    rerender(<FileDropzone file={file} onFileChange={onFileChange} />);
    const replacement = new File(["replacement"], "second.csv");
    drop(replacement);
    expect(onFileChange).toHaveBeenCalledExactlyOnceWith(replacement);
  });

  it("removes the selected file", () => {
    const onFileChange = vi.fn();
    render(<FileDropzone file={new File(["synthetic"], "test.csv")} onFileChange={onFileChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove File" }));
    expect(onFileChange).toHaveBeenCalledWith(null);
  });
});
