import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FileDropzone } from "./file-dropzone";

describe("native import file dropzone", () => {
  it("accepts a drop and ignores replacements while loading or a file is selected", () => {
    const onFileChange = vi.fn();
    const file = new File(["synthetic"], "test.csv");
    const { container, rerender } = render(
      <FileDropzone file={null} onFileChange={onFileChange} />,
    );
    const drop = () => fireEvent.drop(container.firstChild!, { dataTransfer: { files: [file] } });
    drop();
    expect(onFileChange).toHaveBeenCalledWith(file);
    onFileChange.mockClear();
    rerender(<FileDropzone file={null} isLoading onFileChange={onFileChange} />);
    drop();
    rerender(<FileDropzone file={file} onFileChange={onFileChange} />);
    drop();
    expect(onFileChange).not.toHaveBeenCalled();
  });

  it("removes the selected file", () => {
    const onFileChange = vi.fn();
    render(<FileDropzone file={new File(["synthetic"], "test.csv")} onFileChange={onFileChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove File" }));
    expect(onFileChange).toHaveBeenCalledWith(null);
  });
});
