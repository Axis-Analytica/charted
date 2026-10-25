import { describe, expect, test } from "vitest";
import { formatDate, formatNumberWith, formatTextWith, formatValue, isDateFormat, toDate } from "./index";

const en = "en-US";

describe("number formats", () => {
  test.each([
    // [value, format, expected]
    [1234.567, "0", "1235"],
    [1234.567, "0.00", "1234.57"],
    [1234.567, "#,0", "1,235"],
    [1234.567, "#,##0.00", "1,234.57"],
    [1234.5, "#,0.0#", "1,234.5"],
    [1234.567, "#,0.0#", "1,234.57"],
    [7, "000", "007"],
    [0.5, "#.00", ".50"],
    [0.5, "0.00", "0.50"],
    [0.256, "0%", "26%"],
    [0.256, "0.0%", "25.6%"],
    [0.256, "0.0 %", "25.6 %"],
    [25.6, "0.0\\%", "25.6%"],
    [-1234, "#,0", "-1,234"],
    [-1234, "\\$#,0", "-$1,234"],
    [1234, "\\$#,0.00;(\\$#,0.00)", "$1,234.00"],
    [-1234, "\\$#,0.00;(\\$#,0.00)", "($1,234.00)"],
    [0, "#,0;-#,0;\"zero\"", "zero"],
    [-0.001, "0.00;(0.00)", "0.00"],
    [-0.001, "0.00", "0.00"],
    [1234567, "#,0,", "1,235"],
    [1234567, "#,0.0,,\"M\"", "1.2M"],
    [1234567, "#,0,K", "1,235K"],
    [12345, "0.00E+00", "1.23E+04"],
    [0.000123, "0.0E+0", "1.2E-4"],
    [9.996, "0.00E+00", "1.00E+01"],
    [42, "\"Total: \"0", "Total: 42"],
    [42, "0\" items\"", "42 items"],
    [42, "0 \"kr.\"", "42 kr."],
    [42, "[Red]0", "42"],
    [42, "[$€-407] #,0.00", "€ 42.00"],
    [42, "0_)", "42 "],
    [1234.5, "General", "1,234.5"],
    [1234.5, "Fixed", "1234.50"],
    [1234.5, "Standard", "1,234.50"],
    [0.1234, "Percent", "12.34%"],
    [1234.5, "Scientific", "1.23E+03"],
    [1, "Yes/No", "Yes"],
    [0, "True/False", "False"],
  ])("%s with %s → %s", (value, format, expected) => {
    expect(formatNumberWith(value, format, en)).toBe(expected);
  });

  test("separators follow the locale", () => {
    expect(formatNumberWith(1234.5, "#,0.00", "da-DK")).toBe("1.234,50");
    expect(formatNumberWith(0.256, "0.0%", "da-DK")).toBe("25,6%");
  });

  test("named Currency uses the locale's currency", () => {
    expect(formatNumberWith(12.5, "Currency", "en-US")).toBe("$12.50");
    expect(formatNumberWith(12.5, "Currency", "da-DK")).toMatch(/12,50\s*kr\./);
  });

  test("non-finite values", () => {
    expect(formatNumberWith(Infinity, "0", en)).toBe("∞");
    expect(formatNumberWith(Number.NaN, "0", en)).toBe("NaN");
  });
});

describe("date formats", () => {
  const d = new Date(Date.UTC(2025, 2, 7, 14, 5, 9, 45)); // Friday 7 March 2025 14:05:09.045

  test.each([
    ["yyyy-mm-dd", "2025-03-07"],
    ["dd/mm/yyyy", "07/03/2025"],
    ["m/d/yy", "3/7/25"],
    ["mmm d, yyyy", "Mar 7, 2025"],
    ["mmmm yyyy", "March 2025"],
    ["dddd", "Friday"],
    ["ddd d mmm", "Fri 7 Mar"],
    ["hh:mm:ss", "14:05:09"],
    ["h:mm AM/PM", "2:05 PM"],
    ["hh:nn", "14:05"],
    ["yyyy-mm-dd hh:mm", "2025-03-07 14:05"],
    ["\"Q\"q yyyy", "Q1 2025"],
    // .NET style: MM is the month, mm minutes, HH 24-hour, tt AM/PM.
    ["MM/dd/yyyy", "03/07/2025"],
    ["yyyy-MM-dd HH:mm:ss.fff", "2025-03-07 14:05:09.045"],
    ["h:mm tt", "2:05 PM"],
    ["dd MMM yyyy", "07 Mar 2025"],
  ])("%s → %s", (format, expected) => {
    expect(formatDate(d, format, en)).toBe(expected);
  });

  test("named formats", () => {
    expect(formatDate(d, "Short Date", en)).toBe("3/7/25");
    expect(formatDate(d, "Long Date", en)).toBe("Friday, March 7, 2025");
    expect(formatDate(d, "mmmm", "da-DK")).toBe("marts");
  });

  test("date format detection", () => {
    expect(isDateFormat("yyyy-mm-dd")).toBe(true);
    expect(isDateFormat("Long Date")).toBe(true);
    expect(isDateFormat("#,0.00")).toBe(false);
    expect(isDateFormat("0\" days\"")).toBe(false);
  });
});

describe("parsing dates from the core", () => {
  test.each([
    ["2025-03-07", Date.UTC(2025, 2, 7)],
    ["2025-03-07 14:05:09", Date.UTC(2025, 2, 7, 14, 5, 9)],
    ["2025-03-07T14:05:09", Date.UTC(2025, 2, 7, 14, 5, 9)],
    ["14:05:09", Date.UTC(1970, 0, 1, 14, 5, 9)],
  ])("%s", (text, ms) => {
    expect(toDate(text)?.getTime()).toBe(ms);
  });
  test("numbers are epoch milliseconds", () => {
    expect(toDate(0)?.getTime()).toBe(0);
    expect(toDate("not a date")).toBeNull();
  });
});

describe("formatValue", () => {
  test("blank is empty", () => {
    expect(formatValue(null, "Double", "#,0", en)).toBe("");
  });
  test("whole numbers have no grouping by default", () => {
    expect(formatValue(2025n, "Int64", null, en)).toBe("2025");
  });
  test("dates without a format use the locale", () => {
    expect(formatValue(Date.UTC(2025, 2, 7), "Date", null, en)).toBe("3/7/2025");
    expect(formatValue(Date.UTC(2025, 2, 7), "Date", "yyyy-mm-dd", en)).toBe("2025-03-07");
    expect(formatValue("2025-03-07", "Date", "d mmm", en)).toBe("7 Mar");
  });
  test("booleans", () => {
    expect(formatValue(true, "Boolean", null, en)).toBe("True");
    expect(formatValue(false, "Boolean", "Yes/No", en)).toBe("No");
  });
  test("text sections", () => {
    expect(formatValue("abc", "String", "\"[\"@\"]\"", en)).toBe("[abc]");
    expect(formatTextWith("x", "0;0;0;\"<\"@\">\"")).toBe("<x>");
  });
});
