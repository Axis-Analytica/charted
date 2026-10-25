// Relative date filters ("in the last 3 months"): their wording, shared by the filters
// pane and the relative date slicer.
import type { DateUnit } from "../bridge/generated/DateUnit";
import type { RelativePeriod } from "../bridge/generated/RelativePeriod";
import { msg } from "../i18n";

export const PERIODS: { value: RelativePeriod; label: string }[] = [
  { value: "last", label: msg("Last") },
  { value: "this", label: msg("This") },
  { value: "next", label: msg("Next") },
];

export const UNITS: { value: DateUnit; label: string; one: string }[] = [
  { value: "days", label: msg("Days"), one: msg("day") },
  { value: "weeks", label: msg("Weeks"), one: msg("week") },
  { value: "months", label: msg("Months"), one: msg("month") },
  { value: "years", label: msg("Years"), one: msg("year") },
];

type T = (s: string, ...a: (string | number)[]) => string;

/** "in the last 3 months", "in this year", "in the next 7 days". */
export function relativeLabel(period: RelativePeriod, count: number, unit: DateUnit, t: T): string {
  const u = UNITS.find((x) => x.value === unit)!;
  if (period === "this") return t("in this %1", t(u.one));
  const what = count === 1 ? t(u.one) : t(u.label).toLowerCase();
  return period === "last" ? t("in the last %1 %2", count, what) : t("in the next %1 %2", count, what);
}
