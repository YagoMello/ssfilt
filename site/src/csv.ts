export const MAX_CSV_BYTES = 2 * 1024 * 1024;
export const MAX_CSV_ROWS = 50_000;

export interface ParsedCsv {
  times: Float64Array;
  values: Float64Array;
  rowCount: number;
}

export class CsvInputError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CsvInputError";
  }
}

export function parseCsv(text: string): ParsedCsv {
  const byteLength = new TextEncoder().encode(text).byteLength;
  if (byteLength > MAX_CSV_BYTES) {
    throw new CsvInputError("This CSV is larger than 2 MB. Reduce the file size and try again.");
  }

  const lines = text.replace(/^\uFEFF/, "").split(/\r?\n/);
  const times: number[] = [];
  const values: number[] = [];
  let sawData = false;

  for (let index = 0; index < lines.length; index += 1) {
    const rawLine = lines[index]?.trim() ?? "";
    if (rawLine === "") continue;

    if (!sawData && rawLine.toLowerCase().replace(/\s/g, "") === "time,value") {
      sawData = true;
      continue;
    }
    sawData = true;

    const cells = rawLine.split(",");
    if (cells.length !== 2 || cells.some((cell) => cell.trim() === "")) {
      throw new CsvInputError(`Row ${index + 1} must contain exactly two values: time,value.`);
    }

    const time = Number(cells[0]?.trim());
    const value = Number(cells[1]?.trim());
    if (!Number.isFinite(time) || !Number.isFinite(value)) {
      throw new CsvInputError(`Row ${index + 1} contains a non-finite or invalid number.`);
    }

    const previousTime = times.at(-1);
    if (previousTime !== undefined && time <= previousTime) {
      throw new CsvInputError(`Time must increase strictly. Row ${index + 1} is not after the preceding timestamp.`);
    }

    times.push(time);
    values.push(value);
    if (times.length > MAX_CSV_ROWS) {
      throw new CsvInputError("This CSV has more than 50,000 data rows. Reduce the row count and try again.");
    }
  }

  if (times.length < 2) {
    throw new CsvInputError("Add at least two data rows so the filter has an elapsed interval to process.");
  }

  return {
    times: Float64Array.from(times),
    values: Float64Array.from(values),
    rowCount: times.length,
  };
}
