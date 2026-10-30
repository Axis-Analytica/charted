// What the agent's steps say. The core (crates/agent/src/tools) sends each step's
// message in English with its arguments; these are the same messages, so they're found
// for translation and shown in the user's language.
import { msg } from "../i18n";

export const STEP_MESSAGES = [
  msg("Needs data access: %1"),
  msg("Not done: %1"),
  msg("Added page \"%1\""),
  msg("Changed page \"%1\""),
  msg("Moved page \"%1\""),
  msg("Removed page \"%1\""),
  msg("Added %1 \"%2\""),
  msg("Changed %1 \"%2\""),
  msg("Removed %1"),
  msg("Arranged page \"%1\""),
  msg("Filtered the report: %1"),
  msg("Filtered page \"%1\": %2"),
  msg("Filtered \"%1\": %2"),
  msg("Set the theme to %1"),
  msg("Added bookmark \"%1\""),
  msg("Updated bookmark \"%1\""),
  msg("Renamed bookmark \"%1\""),
  msg("Removed bookmark \"%1\""),
  msg("Added measure %1"),
  msg("Changed measure %1"),
  msg("Removed measure %1"),
  msg("Added column %1"),
  msg("Changed column %1"),
  msg("Removed column %1"),
  msg("Added table %1"),
  msg("Changed table %1"),
  msg("Removed table %1"),
  msg("Added relationship %1"),
  msg("Changed relationship %1"),
  msg("Removed relationship %1"),
  msg("Added hierarchy %1"),
  msg("Changed hierarchy %1"),
  msg("Removed hierarchy %1"),
  msg("Added role \"%1\""),
  msg("Changed role \"%1\""),
  msg("Removed role \"%1\""),
  msg("Added calculation group %1"),
  msg("Changed calculation group %1"),
  msg("Added field parameter %1"),
  msg("Added parameter %1"),
];

/** Agent undo labels ("Agent: build me a page") as shown: the request stays as typed. */
export function agentLabel(label: string, t: (s: string, ...a: string[]) => string): string | null {
  return label.startsWith("Agent: ") ? t("Agent: %1", label.slice(7)) : null;
}
