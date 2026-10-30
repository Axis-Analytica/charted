// Which report view a component is in: 0, the one picked in the rail; 1, the second report
// shown beside it (a layout's split), which keeps its own page.
import { createContext } from "react";

export const ReportSlot = createContext<0 | 1>(0);
