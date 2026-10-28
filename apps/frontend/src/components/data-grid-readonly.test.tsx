import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { FormattingProvider, useDataGrid } from "@wealthfolio/ui";
import type { ColumnDef } from "@tanstack/react-table";
import { describe, expect, it, vi } from "vitest";

interface Entry {
  id: string;
  amount: string;
  locked: boolean;
}
const data: Entry[] = [
  { id: "hidden", amount: "1", locked: false },
  { id: "visible", amount: "2", locked: false },
  { id: "locked", amount: "3", locked: true },
];
const columns: ColumnDef<Entry>[] = [
  { accessorKey: "id", meta: { readOnly: true } },
  {
    accessorKey: "amount",
    meta: { cell: { variant: "number", valueType: "string" }, readOnly: (row) => row.locked },
  },
];
function Harness({ onChange }: { onChange: (rows: Entry[]) => void }) {
  const grid = useDataGrid({
    data,
    columns,
    getRowId: (row) => row.id,
    onDataChange: onChange,
    initialState: {
      sorting: [{ id: "amount", desc: true }],
      columnFilters: [{ id: "id", value: "visible" }],
    },
  });
  return (
    <div ref={grid.dataGridRef}>
      <button
        onClick={() =>
          grid.tableMeta.onDataUpdate?.({
            rowIndex: 0,
            columnId: "amount",
            value: "9007199254740993.12",
          })
        }
      >
        Update visible
      </button>
      <button onClick={() => grid.table.setColumnFilters([])}>Show all</button>
      <button onClick={() => grid.tableMeta.onCellEditingStart?.(0, "amount")}>Edit locked</button>
      <button
        onClick={() =>
          grid.tableMeta.onDataUpdate?.({ rowIndex: 0, columnId: "amount", value: "9" })
        }
      >
        Update locked
      </button>
      <output data-testid="first">{grid.table.getRowModel().rows[0]?.id}</output>
      <output>{grid.editingCell ? "editing" : "readonly"}</output>
    </div>
  );
}
describe("DataGrid immutable cells", () => {
  it("updates the original identity after sorting/filtering without dropping hidden rows", () => {
    const onChange = vi.fn();
    render(
      <FormattingProvider locale="en-US">
        <Harness onChange={onChange} />
      </FormattingProvider>,
    );
    fireEvent.click(screen.getByText("Update visible"));
    expect(onChange.mock.calls[0][0]).toEqual([
      data[0],
      { ...data[1], amount: "9007199254740993.12" },
      data[2],
    ]);
  });
  it("does not start editing or modify locked records", async () => {
    const onChange = vi.fn();
    render(
      <FormattingProvider locale="en-US">
        <Harness onChange={onChange} />
      </FormattingProvider>,
    );
    fireEvent.click(screen.getByText("Show all"));
    await waitFor(() => expect(screen.getByTestId("first")).toHaveTextContent("locked"));
    fireEvent.click(screen.getByText("Edit locked"));
    expect(screen.getByText("readonly")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Update locked"));
    expect(onChange).not.toHaveBeenCalled();
  });
});
